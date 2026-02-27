use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::cache::{Cache, CacheLookup};
use crate::parser::{parse_dir_and_cache, parse_dir_for_name, FileStat, FtpFsConfig, ParsedEntry};
use crate::path_utils::{get_full_path, get_fulldir_path, PathContext};
use crate::transport::{FtpTransport, TransportError};

pub struct FtpFsService {
    pub transport: Arc<dyn FtpTransport>,
    pub cache: Cache,
    pub parser_cfg: FtpFsConfig,
    pub path_ctx: PathContext,
    open_writes: Mutex<HashMap<String, Vec<u8>>>,
}

impl FtpFsService {
    pub fn new(
        transport: Arc<dyn FtpTransport>,
        cache: Cache,
        parser_cfg: FtpFsConfig,
        path_ctx: PathContext,
    ) -> Self {
        Self {
            transport,
            cache,
            parser_cfg,
            path_ctx,
            open_writes: Mutex::new(HashMap::new()),
        }
    }

    pub fn readdir(&self, path: &str) -> Result<Vec<ParsedEntry>, i32> {
        if let CacheLookup::Hit(names) = self.cache.get_dir(path) {
            let mut out = Vec::new();
            for name in names {
                let full = format!("{}{}", path, name);
                if let CacheLookup::Hit(st) = self.cache.get_attr(&full) {
                    out.push(ParsedEntry {
                        name,
                        full_path: full,
                        stat: st,
                        symlink_target: None,
                    });
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }

        let url = get_fulldir_path(path, &self.path_ctx);
        let list = self.transport.list(&url).map_err(map_err)?;
        Ok(parse_dir_and_cache(
            &list,
            path,
            &self.parser_cfg,
            &self.cache,
        ))
    }

    pub fn getattr(&self, path: &str) -> Result<FileStat, i32> {
        match self.cache.get_attr(path) {
            CacheLookup::Hit(st) => return Ok(st),
            CacheLookup::NotFound => return Err(-2),
            CacheLookup::Miss => {}
        }

        let parent = match path.rsplit_once('/') {
            Some(("", name)) => ("/", name),
            Some((p, name)) => (p, name),
            None => ("/", path),
        };

        let url = get_fulldir_path(parent.0, &self.path_ctx);
        let list = self.transport.list(&url).map_err(map_err)?;
        if let Some(entry) = parse_dir_for_name(
            &list,
            &format!("{}/", parent.0.trim_end_matches('/')),
            parent.1,
            &self.parser_cfg,
        ) {
            self.cache.add_attr(path, Some(entry.stat.clone()));
            Ok(entry.stat)
        } else {
            self.cache.add_attr(path, None);
            Err(-2)
        }
    }

    pub fn readlink(&self, path: &str) -> Result<String, i32> {
        if let CacheLookup::Hit(link) = self.cache.get_link(path) {
            return Ok(link);
        }
        let url = get_full_path(path, &self.path_ctx);
        let link = self.transport.read_link(&url).map_err(map_err)?;
        self.cache.add_link(path, &link, link.len() + 1);
        Ok(link)
    }

    pub fn read(&self, path: &str, offset: u64, size: usize) -> Result<Vec<u8>, i32> {
        let url = get_full_path(path, &self.path_ctx);
        self.transport
            .read_file_range(&url, offset, size)
            .map_err(map_err)
    }

    pub fn open_write(&self, path: &str) -> Result<(), i32> {
        self.open_writes
            .lock()
            .map_err(|_| -5)?
            .insert(path.to_string(), Vec::new());
        Ok(())
    }

    pub fn write(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize, i32> {
        let mut writes = self.open_writes.lock().map_err(|_| -5)?;
        let buf = writes.get_mut(path).ok_or(-9)?;
        let off = offset as usize;
        if buf.len() < off {
            buf.resize(off, 0);
        }
        if buf.len() < off + data.len() {
            buf.resize(off + data.len(), 0);
        }
        buf[off..off + data.len()].copy_from_slice(data);
        Ok(data.len())
    }

    pub fn flush(&self, path: &str) -> Result<(), i32> {
        let data = {
            let writes = self.open_writes.lock().map_err(|_| -5)?;
            writes.get(path).cloned().ok_or(-9)?
        };
        let url = get_full_path(path, &self.path_ctx);
        self.transport.write_file(&url, &data).map_err(map_err)?;
        self.cache.invalidate(path);
        Ok(())
    }

    pub fn release(&self, path: &str) -> Result<(), i32> {
        self.open_writes.lock().map_err(|_| -5)?.remove(path);
        Ok(())
    }
}

fn map_err(err: TransportError) -> i32 {
    match err {
        TransportError::NotFound => -2,
        TransportError::Io(_) => -5,
    }
}
