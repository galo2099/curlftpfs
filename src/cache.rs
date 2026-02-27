use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::parser::FileStat;

pub const DEFAULT_CACHE_TIMEOUT: u64 = 10;
pub const MAX_CACHE_SIZE: usize = 10_000;
pub const MIN_CACHE_CLEAN_INTERVAL: u64 = 5;
pub const CACHE_CLEAN_INTERVAL: u64 = 60;

#[derive(Debug, Clone)]
struct CacheNode {
    stat: Option<FileStat>,
    not_found: bool,
    stat_valid_until: Option<Instant>,
    dir: Option<Vec<String>>,
    dir_valid_until: Option<Instant>,
    link: Option<String>,
    link_valid_until: Option<Instant>,
    valid_until: Option<Instant>,
}

impl CacheNode {
    fn new() -> Self {
        Self {
            stat: None,
            not_found: false,
            stat_valid_until: None,
            dir: None,
            dir_valid_until: None,
            link: None,
            link_valid_until: None,
            valid_until: None,
        }
    }

    fn touch_valid(&mut self, until: Instant) {
        self.valid_until = Some(self.valid_until.map_or(until, |prev| prev.max(until)));
    }
}

#[derive(Debug)]
struct Inner {
    on: bool,
    stat_timeout: Duration,
    dir_timeout: Duration,
    link_timeout: Duration,
    last_cleaned: Instant,
    table: HashMap<String, CacheNode>,
}

#[derive(Debug)]
pub struct Cache {
    inner: Mutex<Inner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheLookup<T> {
    Hit(T),
    NotFound,
    Miss,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new(
            DEFAULT_CACHE_TIMEOUT,
            DEFAULT_CACHE_TIMEOUT,
            DEFAULT_CACHE_TIMEOUT,
            true,
        )
    }
}

impl Cache {
    pub fn new(
        stat_timeout_secs: u64,
        dir_timeout_secs: u64,
        link_timeout_secs: u64,
        on: bool,
    ) -> Self {
        Self {
            inner: Mutex::new(Inner {
                on,
                stat_timeout: Duration::from_secs(stat_timeout_secs),
                dir_timeout: Duration::from_secs(dir_timeout_secs),
                link_timeout: Duration::from_secs(link_timeout_secs),
                last_cleaned: Instant::now(),
                table: HashMap::new(),
            }),
        }
    }

    pub fn add_attr(&self, path: &str, stbuf: Option<FileStat>) {
        let mut inner = self.inner.lock().expect("cache lock poisoned");
        if !inner.on {
            return;
        }
        let now = Instant::now();
        let stat_timeout = inner.stat_timeout;
        let node = inner
            .table
            .entry(path.to_string())
            .or_insert_with(CacheNode::new);
        match stbuf {
            Some(st) => {
                node.stat = Some(st);
                node.not_found = false;
            }
            None => {
                node.stat = None;
                node.not_found = true;
            }
        }
        let expiry = now + stat_timeout;
        node.stat_valid_until = Some(expiry);
        node.touch_valid(expiry);
        clean_if_needed(&mut inner);
    }

    pub fn add_dir(&self, path: &str, dir: Vec<String>) {
        let mut inner = self.inner.lock().expect("cache lock poisoned");
        if !inner.on {
            return;
        }
        let now = Instant::now();
        let dir_timeout = inner.dir_timeout;
        let node = inner
            .table
            .entry(path.to_string())
            .or_insert_with(CacheNode::new);
        node.dir = Some(dir);
        node.not_found = false;
        let expiry = now + dir_timeout;
        node.dir_valid_until = Some(expiry);
        node.touch_valid(expiry);
        clean_if_needed(&mut inner);
    }

    pub fn add_link(&self, path: &str, link: &str, size: usize) {
        let mut inner = self.inner.lock().expect("cache lock poisoned");
        if !inner.on {
            return;
        }
        let now = Instant::now();
        let link_timeout = inner.link_timeout;
        let node = inner
            .table
            .entry(path.to_string())
            .or_insert_with(CacheNode::new);
        let cap = size.saturating_sub(1);
        node.link = Some(link.chars().take(cap).collect());
        node.not_found = false;
        let expiry = now + link_timeout;
        node.link_valid_until = Some(expiry);
        node.touch_valid(expiry);
        clean_if_needed(&mut inner);
    }

    pub fn get_attr(&self, path: &str) -> CacheLookup<FileStat> {
        let inner = self.inner.lock().expect("cache lock poisoned");
        if !inner.on {
            return CacheLookup::Miss;
        }
        let now = Instant::now();
        match inner.table.get(path) {
            Some(node) if node.stat_valid_until.is_some_and(|v| v >= now) => {
                if node.not_found {
                    CacheLookup::NotFound
                } else {
                    node.stat
                        .clone()
                        .map(CacheLookup::Hit)
                        .unwrap_or(CacheLookup::Miss)
                }
            }
            _ => CacheLookup::Miss,
        }
    }

    pub fn get_dir(&self, path: &str) -> CacheLookup<Vec<String>> {
        let inner = self.inner.lock().expect("cache lock poisoned");
        if !inner.on {
            return CacheLookup::Miss;
        }
        let now = Instant::now();
        match inner.table.get(path) {
            Some(node) if node.dir_valid_until.is_some_and(|v| v >= now) => node
                .dir
                .clone()
                .map(CacheLookup::Hit)
                .unwrap_or(CacheLookup::Miss),
            _ => CacheLookup::Miss,
        }
    }

    pub fn get_link(&self, path: &str) -> CacheLookup<String> {
        let inner = self.inner.lock().expect("cache lock poisoned");
        if !inner.on {
            return CacheLookup::Miss;
        }
        let now = Instant::now();
        match inner.table.get(path) {
            Some(node) if node.link_valid_until.is_some_and(|v| v >= now) => node
                .link
                .clone()
                .map(CacheLookup::Hit)
                .unwrap_or(CacheLookup::Miss),
            _ => CacheLookup::Miss,
        }
    }

    pub fn invalidate(&self, path: &str) {
        let mut inner = self.inner.lock().expect("cache lock poisoned");
        inner.table.remove(path);
    }

    pub fn invalidate_dir(&self, path: &str) {
        let mut inner = self.inner.lock().expect("cache lock poisoned");
        inner.table.remove(path);
        if let Some(parent) = parent_path(path) {
            inner.table.remove(&parent);
        }
    }

    pub fn rename_invalidate(&self, from: &str, to: &str) {
        let mut inner = self.inner.lock().expect("cache lock poisoned");
        inner.table.remove(from);
        inner.table.remove(to);
        if let Some(parent) = parent_path(from) {
            inner.table.remove(&parent);
        }
        if let Some(parent) = parent_path(to) {
            inner.table.remove(&parent);
        }
    }
}

fn parent_path(path: &str) -> Option<String> {
    match path.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(idx) => Some(path[..idx].to_string()),
        None => None,
    }
}

fn clean_if_needed(inner: &mut Inner) {
    let now = Instant::now();
    if now.duration_since(inner.last_cleaned) >= Duration::from_secs(MIN_CACHE_CLEAN_INTERVAL)
        && (inner.table.len() > MAX_CACHE_SIZE
            || now.duration_since(inner.last_cleaned) >= Duration::from_secs(CACHE_CLEAN_INTERVAL))
    {
        inner
            .table
            .retain(|_, node| node.valid_until.is_some_and(|v| v >= now));
        inner.last_cleaned = now;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheOptions {
    pub on: bool,
    pub stat_timeout: u64,
    pub dir_timeout: u64,
    pub link_timeout: u64,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            on: true,
            stat_timeout: DEFAULT_CACHE_TIMEOUT,
            dir_timeout: DEFAULT_CACHE_TIMEOUT,
            link_timeout: DEFAULT_CACHE_TIMEOUT,
        }
    }
}

pub fn parse_cache_options(args: &[String]) -> CacheOptions {
    let mut opts = CacheOptions::default();

    for arg in args {
        if arg == "cache=yes" {
            opts.on = true;
        } else if arg == "cache=no" {
            opts.on = false;
        } else if let Some(v) = arg.strip_prefix("cache_timeout=") {
            if let Ok(secs) = v.parse() {
                opts.stat_timeout = secs;
                opts.dir_timeout = secs;
                opts.link_timeout = secs;
            }
        } else if let Some(v) = arg.strip_prefix("cache_stat_timeout=") {
            if let Ok(secs) = v.parse() {
                opts.stat_timeout = secs;
            }
        } else if let Some(v) = arg.strip_prefix("cache_dir_timeout=") {
            if let Ok(secs) = v.parse() {
                opts.dir_timeout = secs;
            }
        } else if let Some(v) = arg.strip_prefix("cache_link_timeout=") {
            if let Ok(secs) = v.parse() {
                opts.link_timeout = secs;
            }
        }
    }

    opts
}
