use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyWrite, Request,
};

use crate::cache::Cache;
use crate::parser::{FileStat, FtpFsConfig, ParsedEntry};
use crate::path_utils::PathContext;
use crate::service::FtpFsService;
use crate::transport::CurlTransport;

const ENOENT: i32 = 2;
const TTL: Duration = Duration::from_secs(1);

pub struct FtpFuseFs {
    ftp_site: String,
}

impl FtpFuseFs {
    pub fn new(ftp_site: &str) -> Result<Self, String> {
        if !ftp_site.starts_with("ftp://") {
            return Err("invalid ftp url: must start with ftp://".to_string());
        }
        Ok(Self {
            ftp_site: ftp_site.to_string(),
        })
    }

    pub fn mount(self, mountpoint: &str) -> Result<(), String> {
        if !std::path::Path::new(mountpoint).exists() {
            return Err(format!("mountpoint does not exist: {mountpoint}"));
        }

        let fs = LiveFtpFs::new(&self.ftp_site);
        let options = vec![
            MountOption::FSName("curlftpfs-rust".to_string()),
            MountOption::DefaultPermissions,
            MountOption::AutoUnmount,
        ];

        fuser::mount2(fs, mountpoint, &options).map_err(|e| format!("mount failed: {e}"))
    }
}

struct LiveFtpFs {
    service: FtpFsService,
    ino_to_path: HashMap<u64, String>,
    path_to_ino: HashMap<String, u64>,
    next_ino: u64,
}

impl LiveFtpFs {
    fn new(ftp_site: &str) -> Self {
        let service = FtpFsService::new(
            Arc::new(CurlTransport),
            Cache::default(),
            FtpFsConfig::default(),
            PathContext {
                host: ftp_site.to_string(),
                codepage: None,
                iocharset: None,
            },
        );
        let mut ino_to_path = HashMap::new();
        let mut path_to_ino = HashMap::new();
        ino_to_path.insert(1, "/".to_string());
        path_to_ino.insert("/".to_string(), 1);
        Self {
            service,
            ino_to_path,
            path_to_ino,
            next_ino: 2,
        }
    }

    fn ino_for(&mut self, path: &str) -> u64 {
        if let Some(i) = self.path_to_ino.get(path) {
            *i
        } else {
            let i = self.next_ino;
            self.next_ino += 1;
            self.path_to_ino.insert(path.to_string(), i);
            self.ino_to_path.insert(i, path.to_string());
            i
        }
    }

    fn path_for(&self, ino: u64) -> Option<String> {
        self.ino_to_path.get(&ino).cloned()
    }

    fn attr_for(&mut self, path: &str, st: &FileStat) -> FileAttr {
        let ino = self.ino_for(path);
        let kind = match st.mode & 0o170000 {
            0o040000 => FileType::Directory,
            0o120000 => FileType::Symlink,
            _ => FileType::RegularFile,
        };
        FileAttr {
            ino,
            size: st.size,
            blocks: st.blocks,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind,
            perm: (st.mode & 0o777) as u16,
            nlink: st.nlink as u32,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: st.blksize as u32,
        }
    }
}

impl Filesystem for LiveFtpFs {
    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let Some(path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        match self.service.readlink(&path) {
            Ok(link) => reply.data(link.as_bytes()),
            Err(e) => reply.error(-e),
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        let Some(_path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        reply.opened(ino, 0);
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(parent_path) = self.path_for(parent) else {
            reply.error(ENOENT);
            return;
        };
        let entries = match self.service.readdir(&parent_path) {
            Ok(v) => v,
            Err(e) => {
                reply.error(-e);
                return;
            }
        };
        let needle = name.to_string_lossy();
        if let Some(e) = entries.into_iter().find(|e| e.name == needle) {
            let attr = self.attr_for(&e.full_path, &e.stat);
            reply.entry(&TTL, &attr, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        let path = if ino == 1 {
            "/".to_string()
        } else if let Some(p) = self.path_for(ino) {
            p
        } else {
            reply.error(ENOENT);
            return;
        };

        let st = if path == "/" {
            FileStat {
                mode: 0o040755,
                nlink: 2,
                size: 0,
                blksize: 4096,
                blocks: 0,
                mtime_raw: String::new(),
            }
        } else {
            match self.service.getattr(&path) {
                Ok(v) => v,
                Err(e) => {
                    reply.error(-e);
                    return;
                }
            }
        };

        let attr = self.attr_for(&path, &st);
        reply.attr(&TTL, &attr);
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        let entries = match self.service.readdir(&path) {
            Ok(v) => v,
            Err(e) => {
                reply.error(-e);
                return;
            }
        };

        let mut dirents: Vec<(u64, FileType, String)> = vec![
            (ino, FileType::Directory, ".".into()),
            (1, FileType::Directory, "..".into()),
        ];

        for ParsedEntry {
            name,
            full_path,
            stat,
            ..
        } in entries
        {
            let attr = self.attr_for(&full_path, &stat);
            dirents.push((attr.ino, attr.kind, name));
        }

        for (i, (ino, kind, name)) in dirents.into_iter().enumerate().skip(offset as usize) {
            if reply.add(ino, (i + 1) as i64, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        match self
            .service
            .read(&path, offset.max(0) as u64, size as usize)
        {
            Ok(v) => reply.data(&v),
            Err(e) => reply.error(-e),
        }
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        let Some(path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        let _ = self.service.open_write(&path);
        match self.service.write(&path, offset.max(0) as u64, data) {
            Ok(n) => reply.written(n as u32),
            Err(e) => reply.error(-e),
        }
    }

    fn flush(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        _lock_owner: u64,
        reply: ReplyEmpty,
    ) {
        let Some(path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        match self.service.flush(&path) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(-e),
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let Some(path) = self.path_for(ino) else {
            reply.error(ENOENT);
            return;
        };
        match self.service.release(&path) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(-e),
        }
    }
}
