use crate::remote::{ListEntry, Remote};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request, TimeOrNow,
};
use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant, SystemTime},
};
const ROOT: u64 = 1;
const MAX_DIRECTORY_CACHE_ENTRIES: usize = 10_000;

#[derive(Clone)]
struct Node {
    ino: u64,
    path: PathBuf,
    kind: FileType,
    size: u64,
    mtime: SystemTime,
    link_target: Option<String>,
    perm: u16,
    nlink: u32,
}
struct Handle {
    path: PathBuf,
    data: Vec<u8>,
    dirty: bool,
}
struct CachedDirectory {
    entries: Arc<[ListEntry]>,
    cached_at: Instant,
}
struct DirectoryCache {
    timeout: Duration,
    max_entries: usize,
    revision: u64,
    directories: HashMap<PathBuf, CachedDirectory>,
    in_flight: HashSet<PathBuf>,
}
impl DirectoryCache {
    fn new(timeout: Duration) -> Self {
        Self::with_limit(timeout, MAX_DIRECTORY_CACHE_ENTRIES)
    }
    fn with_limit(timeout: Duration, max_entries: usize) -> Self {
        Self {
            timeout,
            max_entries,
            revision: 0,
            directories: HashMap::new(),
            in_flight: HashSet::new(),
        }
    }
    fn get(&mut self, path: &Path, now: Instant) -> Option<Arc<[ListEntry]>> {
        let valid = self
            .directories
            .get(path)
            .is_some_and(|cached| now.saturating_duration_since(cached.cached_at) < self.timeout);
        if valid {
            return self
                .directories
                .get(path)
                .map(|cached| cached.entries.clone());
        }
        self.directories.remove(path);
        None
    }
    fn insert(&mut self, path: PathBuf, entries: Arc<[ListEntry]>, now: Instant, revision: u64) {
        if self.timeout.is_zero() || self.max_entries == 0 || self.revision != revision {
            return;
        }
        self.directories
            .retain(|_, cached| now.saturating_duration_since(cached.cached_at) < self.timeout);
        if !self.directories.contains_key(&path) && self.directories.len() >= self.max_entries {
            if let Some(oldest) = self
                .directories
                .iter()
                .min_by_key(|(_, cached)| cached.cached_at)
                .map(|(path, _)| path.clone())
            {
                self.directories.remove(&oldest);
            }
        }
        self.directories.insert(
            path,
            CachedDirectory {
                entries,
                cached_at: now,
            },
        );
    }
    fn invalidate(&mut self, path: &Path) {
        self.revision = self.revision.wrapping_add(1);
        self.directories.remove(path);
    }
    fn invalidate_tree(&mut self, path: &Path) {
        self.revision = self.revision.wrapping_add(1);
        self.directories
            .retain(|cached_path, _| !cached_path.starts_with(path));
    }
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
    directory_cache: Mutex<DirectoryCache>,
    directory_cache_changed: Condvar,
    ttl: Duration,
    mountpoint: PathBuf,
    transform_symlinks: bool,
}
impl FtpFs {
    pub fn new(
        remote: Remote,
        ttl: Duration,
        mountpoint: PathBuf,
        transform_symlinks: bool,
    ) -> Self {
        let root = Node {
            ino: ROOT,
            path: "/".into(),
            kind: FileType::Directory,
            size: 0,
            mtime: SystemTime::now(),
            link_target: None,
            perm: 0o755,
            nlink: 2,
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
            directory_cache: Mutex::new(DirectoryCache::new(ttl)),
            directory_cache_changed: Condvar::new(),
            ttl,
            mountpoint,
            transform_symlinks,
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
            perm: if n.perm == 0 {
                if n.kind == FileType::Directory {
                    0o755
                } else {
                    0o644
                }
            } else {
                n.perm
            },
            nlink: n.nlink,
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
            n.mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(e.modified.max(0) as u64);
            n.link_target = e.link_target.clone();
            n.perm = e.perm;
            n.nlink = e.nlink;
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
            link_target: e.link_target.clone(),
            perm: e.perm,
            nlink: e.nlink,
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
    fn list_directory(&self, path: &Path) -> io::Result<Arc<[ListEntry]>> {
        let (revision, should_cache) = {
            let mut cache = self.directory_cache.lock().unwrap();
            loop {
                if let Some(entries) = cache.get(path, Instant::now()) {
                    return Ok(entries);
                }
                if cache.timeout.is_zero() {
                    break (cache.revision, false);
                }
                if cache.in_flight.insert(path.to_path_buf()) {
                    break (cache.revision, true);
                }
                cache = self.directory_cache_changed.wait(cache).unwrap();
            }
        };
        let result = self.remote.list(path).map(Arc::<[ListEntry]>::from);
        if should_cache {
            let mut cache = self.directory_cache.lock().unwrap();
            cache.in_flight.remove(path);
            if let Ok(entries) = &result {
                cache.insert(
                    path.to_path_buf(),
                    Arc::clone(entries),
                    Instant::now(),
                    revision,
                );
            }
            self.directory_cache_changed.notify_all();
        }
        result
    }
    fn invalidate_parent_directory(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            self.directory_cache.lock().unwrap().invalidate(parent);
        }
    }
    fn invalidate_path_and_parent(&self, path: &Path) {
        let mut cache = self.directory_cache.lock().unwrap();
        cache.invalidate_tree(path);
        if let Some(parent) = path.parent() {
            cache.invalidate(parent);
        }
    }
    fn invalidate_rename(&self, old: &Path, new: &Path) {
        let mut cache = self.directory_cache.lock().unwrap();
        cache.invalidate_tree(old);
        cache.invalidate_tree(new);
        if let Some(parent) = old.parent() {
            cache.invalidate(parent);
        }
        if let Some(parent) = new.parent() {
            cache.invalidate(parent);
        }
    }
    fn upload_file(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.remote.upload(path, data)?;
        self.invalidate_parent_directory(path);
        Ok(())
    }
    fn remove_node(&self, path: &Path) {
        let mut s = self.state.lock().unwrap();
        if let Some(i) = s.by_path.remove(path) {
            s.by_ino.remove(&i);
        }
    }
    fn rename_node(&self, old: &Path, new: &Path) {
        let mut s = self.state.lock().unwrap();
        let moved: Vec<(PathBuf, PathBuf, u64)> = s
            .by_path
            .iter()
            .filter_map(|(path, ino)| {
                let suffix = path.strip_prefix(old).ok()?;
                let target = if suffix.as_os_str().is_empty() {
                    new.to_path_buf()
                } else {
                    new.join(suffix)
                };
                Some((path.clone(), target, *ino))
            })
            .collect();
        let moved_inodes: Vec<u64> = moved.iter().map(|(_, _, ino)| *ino).collect();
        for (source, _, _) in &moved {
            s.by_path.remove(source);
        }
        for (_, target, ino) in moved {
            if let Some(replaced) = s.by_path.insert(target.clone(), ino) {
                if !moved_inodes.contains(&replaced) {
                    s.by_ino.remove(&replaced);
                }
            }
            if let Some(node) = s.by_ino.get_mut(&ino) {
                node.path = target;
            }
        }
    }
    fn refresh_node(&self, node: &Node) -> io::Result<Option<Node>> {
        if node.ino == ROOT {
            return Ok(Some(node.clone()));
        }
        let Some(parent) = node.path.parent() else {
            return Ok(None);
        };
        let Some(name) = node.path.file_name() else {
            return Ok(None);
        };
        Ok(self
            .list_directory(parent)?
            .iter()
            .find(|entry| OsStr::new(&entry.name) == name)
            .map(|entry| self.intern(node.path.clone(), entry)))
    }
}
impl Filesystem for FtpFs {
    fn getattr(&mut self, _: &Request<'_>, ino: u64, reply: ReplyAttr) {
        match self.node(ino) {
            Some(n) => match self.refresh_node(&n) {
                Ok(Some(n)) => reply.attr(&self.ttl, &Self::attr(&n)),
                Ok(None) => {
                    self.remove_node(&n.path);
                    reply.error(libc::ENOENT)
                }
                Err(e) => reply.error(Self::errno(&e)),
            },
            None => reply.error(libc::ENOENT),
        }
    }
    fn readlink(&mut self, _: &Request<'_>, ino: u64, reply: ReplyData) {
        match self.node(ino) {
            Some(n) if n.kind == FileType::Symlink => match n.link_target {
                Some(target) => {
                    if self.transform_symlinks && target.starts_with('/') {
                        reply.data(
                            self.mountpoint
                                .join(target.trim_start_matches('/'))
                                .as_os_str()
                                .as_encoded_bytes(),
                        )
                    } else {
                        reply.data(target.as_bytes())
                    }
                }
                None => reply.error(libc::EIO),
            },
            Some(_) => reply.error(libc::EINVAL),
            None => reply.error(libc::ENOENT),
        }
    }
    fn statfs(&mut self, _: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
        // FTP has no capacity query. Match the legacy implementation's
        // synthetic, effectively-unlimited statfs result.
        reply.statfs(
            1_999_999_998,
            1_999_999_998,
            1_999_999_998,
            999_999_999,
            999_999_999,
            4096,
            255,
            4096,
        );
    }
    fn lookup(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(p) = self.node(parent) else {
            return reply.error(libc::ENOENT);
        };
        let path = Self::child(&p, name);
        match self.list_directory(&p.path) {
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
        match self.list_directory(&dir.path) {
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
                for e in entries.iter() {
                    let n = self.intern(dir.path.join(&e.name), e);
                    all.push((n.ino, n.kind, OsString::from(&e.name)));
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
            Some((p, d)) => match self.upload_file(&p, &d) {
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
            match self.upload_file(&h.path, &h.data) {
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
        match self.upload_file(&path, &[]) {
            Ok(()) => {
                let e = ListEntry {
                    name: name.to_string_lossy().into(),
                    size: 0,
                    directory: false,
                    symlink: false,
                    modified: 0,
                    link_target: None,
                    perm: 0o644,
                    nlink: 1,
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
    fn mknod(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        let Some(p) = self.node(parent) else {
            return reply.error(libc::ENOENT);
        };
        let path = Self::child(&p, name);
        match self.upload_file(&path, &[]) {
            Ok(()) => {
                let e = ListEntry {
                    name: name.to_string_lossy().into(),
                    size: 0,
                    directory: false,
                    symlink: false,
                    modified: 0,
                    link_target: None,
                    perm: 0o644,
                    nlink: 1,
                };
                let n = self.intern(path, &e);
                reply.entry(&self.ttl, &Self::attr(&n), 0)
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn fsync(&mut self, _: &Request<'_>, _ino: u64, fh: u64, _datasync: bool, reply: ReplyEmpty) {
        let upload = {
            let s = self.state.lock().unwrap();
            s.handles
                .get(&fh)
                .filter(|h| h.dirty)
                .map(|h| (h.path.clone(), h.data.clone()))
        };
        match upload {
            Some((path, data)) => match self.upload_file(&path, &data) {
                Ok(()) => {
                    if let Some(h) = self.state.lock().unwrap().handles.get_mut(&fh) {
                        h.dirty = false;
                    }
                    reply.ok()
                }
                Err(e) => reply.error(Self::errno(&e)),
            },
            None => reply.ok(),
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
            .command(Path::new("/"), format!("MKD {}", path.to_string_lossy()))
        {
            Ok(()) => {
                self.invalidate_path_and_parent(&path);
                let e = ListEntry {
                    name: name.to_string_lossy().into(),
                    size: 0,
                    directory: true,
                    symlink: false,
                    modified: 0,
                    link_target: None,
                    perm: 0o755,
                    nlink: 2,
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
                self.invalidate_rename(&old, &new);
                self.rename_node(&old, &new);
                reply.ok()
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
    fn setattr(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
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
        if let Some(mode) = mode {
            if let Err(e) = self.remote.command(
                Path::new("/"),
                format!(
                    "SITE CHMOD {:03o} {}",
                    mode & 0o7777,
                    n.path.to_string_lossy()
                ),
            ) {
                return reply.error(Self::errno(&e));
            }
            self.invalidate_parent_directory(&n.path);
            n.perm = (mode & 0o7777) as u16;
            self.state.lock().unwrap().by_ino.insert(ino, n.clone());
        }
        if let Some(size) = size {
            match self.remote.download(&n.path).and_then(|mut d| {
                d.resize(size as usize, 0);
                self.upload_file(&n.path, &d)
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
            path.to_string_lossy()
        );
        match self.remote.command(Path::new("/"), cmd) {
            Ok(()) => {
                self.invalidate_path_and_parent(&path);
                self.remove_node(&path);
                reply.ok()
            }
            Err(e) => reply.error(Self::errno(&e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::CurlConfig;

    fn entry(name: &str, directory: bool) -> ListEntry {
        ListEntry {
            name: name.into(),
            size: 0,
            directory,
            symlink: false,
            modified: 0,
            link_target: None,
            perm: if directory { 0o755 } else { 0o644 },
            nlink: if directory { 2 } else { 1 },
        }
    }

    fn cache_directory(cache: &mut DirectoryCache, path: &str, name: &str, now: Instant) {
        cache.insert(
            path.into(),
            Arc::from(vec![entry(name, false)]),
            now,
            cache.revision,
        );
    }

    #[test]
    fn directory_cache_reuses_entries_until_timeout() {
        let start = Instant::now();
        let mut cache = DirectoryCache::new(Duration::from_secs(10));
        cache_directory(&mut cache, "/", "file", start);

        let cached = cache.get(Path::new("/"), start + Duration::from_secs(9));
        assert_eq!(cached.unwrap()[0].name, "file");
        assert!(cache
            .get(Path::new("/"), start + Duration::from_secs(10))
            .is_none());
        assert!(cache.directories.is_empty());
    }

    #[test]
    fn directory_cache_shares_listing_storage() {
        let start = Instant::now();
        let mut cache = DirectoryCache::new(Duration::from_secs(10));
        let entries: Arc<[ListEntry]> = Arc::from(vec![entry("file", false)]);
        cache.insert("/".into(), Arc::clone(&entries), start, cache.revision);

        let cached = cache.get(Path::new("/"), start).unwrap();
        assert!(Arc::ptr_eq(&entries, &cached));
    }

    #[test]
    fn zero_timeout_disables_directory_cache() {
        let start = Instant::now();
        let mut cache = DirectoryCache::new(Duration::ZERO);
        cache_directory(&mut cache, "/", "file", start);

        assert!(cache.get(Path::new("/"), start).is_none());
        assert!(cache.directories.is_empty());
    }

    #[test]
    fn directory_cache_is_bounded() {
        let start = Instant::now();
        let mut cache = DirectoryCache::with_limit(Duration::from_secs(60), 2);
        cache_directory(&mut cache, "/first", "first", start);
        cache_directory(
            &mut cache,
            "/second",
            "second",
            start + Duration::from_secs(1),
        );
        cache_directory(
            &mut cache,
            "/third",
            "third",
            start + Duration::from_secs(2),
        );

        assert_eq!(cache.directories.len(), 2);
        assert!(!cache.directories.contains_key(Path::new("/first")));
        assert!(cache.directories.contains_key(Path::new("/second")));
        assert!(cache.directories.contains_key(Path::new("/third")));
    }

    #[test]
    fn invalidation_discards_in_flight_listing() {
        let start = Instant::now();
        let mut cache = DirectoryCache::new(Duration::from_secs(10));
        let listing_revision = cache.revision;
        cache.invalidate(Path::new("/"));
        cache.insert(
            "/".into(),
            Arc::from(vec![entry("stale", false)]),
            start,
            listing_revision,
        );

        assert!(cache.get(Path::new("/"), start).is_none());
    }

    #[test]
    fn rename_invalidates_parents_and_cached_subtrees() {
        let remote = Remote::new("ftp://example.test/".into(), CurlConfig::default()).unwrap();
        let fs = FtpFs::new(remote, Duration::from_secs(10), "/mnt".into(), false);
        let start = Instant::now();
        {
            let mut cache = fs.directory_cache.lock().unwrap();
            for path in [
                "/",
                "/from",
                "/from/old",
                "/from/old/nested",
                "/to",
                "/to/new",
                "/other",
            ] {
                cache_directory(&mut cache, path, "file", start);
            }
        }

        fs.invalidate_rename(Path::new("/from/old"), Path::new("/to/new"));

        let cache = fs.directory_cache.lock().unwrap();
        assert!(cache.directories.contains_key(Path::new("/")));
        assert!(cache.directories.contains_key(Path::new("/other")));
        for invalidated in ["/from", "/from/old", "/from/old/nested", "/to", "/to/new"] {
            assert!(!cache.directories.contains_key(Path::new(invalidated)));
        }
    }

    #[test]
    fn rename_updates_cached_descendant_paths() {
        let remote = Remote::new("ftp://example.test/".into(), CurlConfig::default()).unwrap();
        let fs = FtpFs::new(remote, Duration::from_secs(1), "/mnt".into(), false);
        let directory = fs.intern("/old".into(), &entry("old", true));
        let child = fs.intern("/old/file".into(), &entry("file", false));

        fs.rename_node(Path::new("/old"), Path::new("/new"));

        let state = fs.state.lock().unwrap();
        assert_eq!(state.by_path.get(Path::new("/new")), Some(&directory.ino));
        assert_eq!(state.by_path.get(Path::new("/new/file")), Some(&child.ino));
        assert_eq!(state.by_ino[&directory.ino].path, Path::new("/new"));
        assert_eq!(state.by_ino[&child.ino].path, Path::new("/new/file"));
        assert!(!state.by_path.contains_key(Path::new("/old")));
        assert!(!state.by_path.contains_key(Path::new("/old/file")));
    }
}
