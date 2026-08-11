use crate::remote::{ListEntry, Remote};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyWrite, Request, TimeOrNow,
};
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
const ROOT: u64 = 1;

#[derive(Clone)]
struct Node {
    ino: u64,
    path: PathBuf,
    kind: FileType,
    size: u64,
    mtime: SystemTime,
}
struct Handle {
    path: PathBuf,
    data: Vec<u8>,
    dirty: bool,
}
struct State {
    next_ino: u64,
    next_fh: u64,
    by_ino: HashMap<u64, Node>,
    by_path: HashMap<PathBuf, u64>,
    handles: HashMap<u64, Handle>,
}
pub struct FtpFs {
    remote: Arc<Remote>,
    state: Mutex<State>,
    ttl: Duration,
}
impl FtpFs {
    pub fn new(remote: Remote, ttl: Duration) -> Self {
        let root = Node {
            ino: ROOT,
            path: "/".into(),
            kind: FileType::Directory,
            size: 0,
            mtime: SystemTime::now(),
        };
        let mut by_ino = HashMap::new();
        by_ino.insert(ROOT, root);
        let mut by_path = HashMap::new();
        by_path.insert("/".into(), ROOT);
        Self {
            remote: Arc::new(remote),
            state: Mutex::new(State {
                next_ino: 2,
                next_fh: 1,
                by_ino,
                by_path,
                handles: HashMap::new(),
            }),
            ttl,
        }
    }
    fn attr(n: &Node) -> FileAttr {
        FileAttr {
            ino: n.ino,
            size: n.size,
            blocks: n.size.div_ceil(512),
            atime: n.mtime,
            mtime: n.mtime,
            ctime: n.mtime,
            crtime: n.mtime,
            kind: n.kind,
            perm: if n.kind == FileType::Directory {
                0o755
            } else {
                0o644
            },
            nlink: if n.kind == FileType::Directory { 2 } else { 1 },
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }
    fn node(&self, ino: u64) -> Option<Node> {
        self.state.lock().unwrap().by_ino.get(&ino).cloned()
    }
    fn intern(&self, path: PathBuf, e: &ListEntry) -> Node {
        let mut s = self.state.lock().unwrap();
        if let Some(ino) = s.by_path.get(&path).copied() {
            let n = s.by_ino.get_mut(&ino).unwrap();
            n.size = e.size;
            n.kind = if e.directory {
                FileType::Directory
            } else if e.symlink {
                FileType::Symlink
            } else {
                FileType::RegularFile
            };
            return n.clone();
        }
        let ino = s.next_ino;
        s.next_ino += 1;
        let n = Node {
            ino,
            path: path.clone(),
            kind: if e.directory {
                FileType::Directory
            } else if e.symlink {
                FileType::Symlink
            } else {
                FileType::RegularFile
            },
            size: e.size,
            mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(e.modified.max(0) as u64),
        };
        s.by_path.insert(path, ino);
        s.by_ino.insert(ino, n.clone());
        n
    }
    fn errno(e: &io::Error) -> i32 {
        match e.kind() {
            io::ErrorKind::NotFound => libc::ENOENT,
            io::ErrorKind::PermissionDenied => libc::EACCES,
            io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut => libc::EHOSTUNREACH,
            _ => libc::EIO,
        }
    }
    fn child(parent: &Node, name: &OsStr) -> PathBuf {
        parent.path.join(name)
    }
    fn remove_node(&self, path: &Path) {
        let mut s = self.state.lock().unwrap();
        if let Some(i) = s.by_path.remove(path) {
            s.by_ino.remove(&i);
        }
    }
}
impl Filesystem for FtpFs {
    fn getattr(&mut self, _: &Request<'_>, ino: u64, reply: ReplyAttr) {
        match self.node(ino) {
            Some(n) => reply.attr(&self.ttl, &Self::attr(&n)),
            None => reply.error(libc::ENOENT),
        }
    }
    fn lookup(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(p) = self.node(parent) else {
            return reply.error(libc::ENOENT);
        };
        let path = Self::child(&p, name);
        if let Some(i) = self.state.lock().unwrap().by_path.get(&path).copied() {
            let n = self.node(i).unwrap();
            return reply.entry(&self.ttl, &Self::attr(&n), 0);
        }
        match self.remote.list(&p.path) {
            Ok(entries) => {
                if let Some(e) = entries.iter().find(|e| OsStr::new(&e.name) == name) {
                    let n = self.intern(path, e);
                    reply.entry(&self.ttl, &Self::attr(&n), 0)
                } else {
                    reply.error(libc::ENOENT)
                }
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn readdir(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(dir) = self.node(ino) else {
            return reply.error(libc::ENOENT);
        };
        match self.remote.list(&dir.path) {
            Ok(entries) => {
                let parent = dir
                    .path
                    .parent()
                    .and_then(|p| self.state.lock().unwrap().by_path.get(p).copied())
                    .unwrap_or(ROOT);
                let mut all = vec![
                    (ROOT, FileType::Directory, OsString::from(".")),
                    (parent, FileType::Directory, OsString::from("..")),
                ];
                for e in entries {
                    let n = self.intern(dir.path.join(&e.name), &e);
                    all.push((n.ino, n.kind, e.name.into()));
                }
                for (i, (ino, kind, name)) in all.into_iter().enumerate().skip(offset as usize) {
                    if reply.add(ino, (i + 1) as i64, kind, name) {
                        break;
                    }
                }
                reply.ok()
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn open(&mut self, _: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        let Some(n) = self.node(ino) else {
            return reply.error(libc::ENOENT);
        };
        let data = if flags & libc::O_TRUNC != 0 {
            Ok(Vec::new())
        } else {
            self.remote.download(&n.path)
        };
        match data {
            Ok(data) => {
                let mut s = self.state.lock().unwrap();
                let fh = s.next_fh;
                s.next_fh += 1;
                s.handles.insert(
                    fh,
                    Handle {
                        path: n.path,
                        data,
                        dirty: flags & libc::O_TRUNC != 0,
                    },
                );
                reply.opened(fh, 0)
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn read(
        &mut self,
        _: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let s = self.state.lock().unwrap();
        let Some(h) = s.handles.get(&fh) else {
            return reply.error(libc::EBADF);
        };
        let start = (offset.max(0) as usize).min(h.data.len());
        let end = (start + size as usize).min(h.data.len());
        reply.data(&h.data[start..end])
    }
    fn write(
        &mut self,
        _: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyWrite,
    ) {
        let mut s = self.state.lock().unwrap();
        let Some(h) = s.handles.get_mut(&fh) else {
            return reply.error(libc::EBADF);
        };
        let start = offset.max(0) as usize;
        if h.data.len() < start {
            h.data.resize(start, 0)
        }
        if h.data.len() < start + data.len() {
            h.data.resize(start + data.len(), 0)
        }
        h.data[start..start + data.len()].copy_from_slice(data);
        h.dirty = true;
        reply.written(data.len() as u32)
    }
    fn flush(&mut self, _: &Request<'_>, _ino: u64, fh: u64, _lock: u64, reply: ReplyEmpty) {
        let upload = {
            let s = self.state.lock().unwrap();
            s.handles
                .get(&fh)
                .filter(|h| h.dirty)
                .map(|h| (h.path.clone(), h.data.clone()))
        };
        match upload {
            Some((p, d)) => match self.remote.upload(&p, &d) {
                Ok(()) => {
                    if let Some(h) = self.state.lock().unwrap().handles.get_mut(&fh) {
                        h.dirty = false
                    }
                    reply.ok()
                }
                Err(e) => reply.error(Self::errno(&e)),
            },
            None => reply.ok(),
        }
    }
    fn release(
        &mut self,
        _: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let h = self.state.lock().unwrap().handles.remove(&fh);
        if let Some(h) = h.filter(|h| h.dirty) {
            match self.remote.upload(&h.path, &h.data) {
                Ok(()) => reply.ok(),
                Err(e) => reply.error(Self::errno(&e)),
            }
        } else {
            reply.ok()
        }
    }
    fn create(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let Some(p) = self.node(parent) else {
            return reply.error(libc::ENOENT);
        };
        let path = Self::child(&p, name);
        match self.remote.upload(&path, &[]) {
            Ok(()) => {
                let e = ListEntry {
                    name: name.to_string_lossy().into(),
                    size: 0,
                    directory: false,
                    symlink: false,
                    modified: 0,
                };
                let n = self.intern(path.clone(), &e);
                let mut s = self.state.lock().unwrap();
                let fh = s.next_fh;
                s.next_fh += 1;
                s.handles.insert(
                    fh,
                    Handle {
                        path,
                        data: Vec::new(),
                        dirty: false,
                    },
                );
                reply.created(&self.ttl, &Self::attr(&n), 0, fh, 0)
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn mkdir(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let Some(p) = self.node(parent) else {
            return reply.error(libc::ENOENT);
        };
        let path = Self::child(&p, name);
        match self
            .remote
            .command(&p.path, format!("MKD {}", name.to_string_lossy()))
        {
            Ok(()) => {
                let e = ListEntry {
                    name: name.to_string_lossy().into(),
                    size: 0,
                    directory: true,
                    symlink: false,
                    modified: 0,
                };
                let n = self.intern(path, &e);
                reply.entry(&self.ttl, &Self::attr(&n), 0)
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn unlink(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        self.delete(parent, name, false, reply)
    }
    fn rmdir(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        self.delete(parent, name, true, reply)
    }
    fn rename(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let (Some(p), Some(np)) = (self.node(parent), self.node(newparent)) else {
            return reply.error(libc::ENOENT);
        };
        let old = Self::child(&p, name);
        let new = Self::child(&np, newname);
        let cmd = format!(
            "RNFR {}\nRNTO {}",
            old.to_string_lossy(),
            new.to_string_lossy()
        );
        match self.remote.command(Path::new("/"), cmd) {
            Ok(()) => {
                self.remove_node(&old);
                reply.ok()
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn setattr(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let Some(mut n) = self.node(ino) else {
            return reply.error(libc::ENOENT);
        };
        if let Some(size) = size {
            match self.remote.download(&n.path).and_then(|mut d| {
                d.resize(size as usize, 0);
                self.remote.upload(&n.path, &d)
            }) {
                Ok(()) => {
                    n.size = size;
                    self.state.lock().unwrap().by_ino.insert(ino, n.clone());
                }
                Err(e) => return reply.error(Self::errno(&e)),
            }
        }
        reply.attr(&self.ttl, &Self::attr(&n))
    }
}
impl FtpFs {
    fn delete(&self, parent: u64, name: &OsStr, dir: bool, reply: ReplyEmpty) {
        let Some(p) = self.node(parent) else {
            return reply.error(libc::ENOENT);
        };
        let path = Self::child(&p, name);
        let cmd = format!(
            "{} {}",
            if dir { "RMD" } else { "DELE" },
            name.to_string_lossy()
        );
        match self.remote.command(&p.path, cmd) {
            Ok(()) => {
                self.remove_node(&path);
                reply.ok()
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
}
