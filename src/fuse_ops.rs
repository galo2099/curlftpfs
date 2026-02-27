use std::sync::atomic::{AtomicBool, Ordering};

use crate::parser::{FileStat, ParsedEntry};
use crate::service::FtpFsService;

pub trait FuseLikeOps {
    fn init(&self);
    fn destroy(&self);
    fn getattr(&self, path: &str) -> Result<FileStat, i32>;
    fn readdir(&self, path: &str) -> Result<Vec<ParsedEntry>, i32>;
    fn readlink(&self, path: &str) -> Result<String, i32>;
    fn read(&self, path: &str, offset: u64, size: usize) -> Result<Vec<u8>, i32>;
    fn open_write(&self, path: &str) -> Result<(), i32>;
    fn write(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize, i32>;
    fn flush(&self, path: &str) -> Result<(), i32>;
    fn release(&self, path: &str) -> Result<(), i32>;
}

pub struct FuseAdapter {
    pub mounted: AtomicBool,
    pub service: FtpFsService,
}

impl FuseAdapter {
    pub fn new(service: FtpFsService) -> Self {
        Self {
            mounted: AtomicBool::new(false),
            service,
        }
    }
}

impl FuseLikeOps for FuseAdapter {
    fn init(&self) {
        self.mounted.store(true, Ordering::SeqCst);
    }

    fn destroy(&self) {
        self.mounted.store(false, Ordering::SeqCst);
    }

    fn getattr(&self, path: &str) -> Result<FileStat, i32> {
        self.service.getattr(path)
    }

    fn readdir(&self, path: &str) -> Result<Vec<ParsedEntry>, i32> {
        self.service.readdir(path)
    }

    fn readlink(&self, path: &str) -> Result<String, i32> {
        self.service.readlink(path)
    }

    fn read(&self, path: &str, offset: u64, size: usize) -> Result<Vec<u8>, i32> {
        self.service.read(path, offset, size)
    }

    fn open_write(&self, path: &str) -> Result<(), i32> {
        self.service.open_write(path)
    }

    fn write(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize, i32> {
        self.service.write(path, offset, data)
    }

    fn flush(&self, path: &str) -> Result<(), i32> {
        self.service.flush(path)
    }

    fn release(&self, path: &str) -> Result<(), i32> {
        self.service.release(path)
    }
}
