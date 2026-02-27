use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use curl::easy::{Easy2, Handler, ReadError, WriteError};
use curl::multi::Multi;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    NotFound,
    Io(String),
}

pub trait FtpTransport: Send + Sync {
    fn list(&self, dir_url: &str) -> Result<String, TransportError>;
    fn read_link(&self, path_url: &str) -> Result<String, TransportError>;
    fn read_file_range(
        &self,
        path_url: &str,
        offset: u64,
        size: usize,
    ) -> Result<Vec<u8>, TransportError>;
    fn write_file(&self, path_url: &str, data: &[u8]) -> Result<(), TransportError>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryTransport {
    pub listings: HashMap<String, String>,
    pub links: HashMap<String, String>,
    pub files: HashMap<String, Vec<u8>>,
}

impl FtpTransport for std::sync::Mutex<MemoryTransport> {
    fn list(&self, dir_url: &str) -> Result<String, TransportError> {
        self.lock()
            .map_err(|_| TransportError::Io("poisoned".into()))?
            .listings
            .get(dir_url)
            .cloned()
            .ok_or(TransportError::NotFound)
    }

    fn read_link(&self, path_url: &str) -> Result<String, TransportError> {
        self.lock()
            .map_err(|_| TransportError::Io("poisoned".into()))?
            .links
            .get(path_url)
            .cloned()
            .ok_or(TransportError::NotFound)
    }

    fn read_file_range(
        &self,
        path_url: &str,
        offset: u64,
        size: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let guard = self
            .lock()
            .map_err(|_| TransportError::Io("poisoned".into()))?;
        let data = guard.files.get(path_url).ok_or(TransportError::NotFound)?;
        let start = offset as usize;
        if start >= data.len() {
            return Ok(Vec::new());
        }
        let end = (start + size).min(data.len());
        Ok(data[start..end].to_vec())
    }

    fn write_file(&self, path_url: &str, data: &[u8]) -> Result<(), TransportError> {
        self.lock()
            .map_err(|_| TransportError::Io("poisoned".into()))?
            .files
            .insert(path_url.to_string(), data.to_vec());
        Ok(())
    }
}

thread_local! {
    static CURL_MULTI: RefCell<Multi> = RefCell::new(Multi::new());
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CurlTransport;

impl CurlTransport {
    fn run_transfer<H: Handler>(&self, easy: Easy2<H>) -> Result<Easy2<H>, TransportError> {
        CURL_MULTI.with(|multi_cell| {
            let multi = multi_cell.borrow_mut();
            let handle = multi
                .add2(easy)
                .map_err(|e| TransportError::Io(e.to_string()))?;

            let mut transfer_result = None;
            loop {
                let running = multi
                    .perform()
                    .map_err(|e| TransportError::Io(e.to_string()))?;

                multi.messages(|msg| {
                    if transfer_result.is_none() {
                        transfer_result = msg.result_for2(&handle);
                    }
                });

                if transfer_result.is_some() && running == 0 {
                    break;
                }

                if running > 0 {
                    multi
                        .wait(&mut [], Duration::from_millis(50))
                        .map_err(|e| TransportError::Io(e.to_string()))?;
                } else if transfer_result.is_none() {
                    break;
                }
            }

            let easy = multi
                .remove2(handle)
                .map_err(|e| TransportError::Io(e.to_string()))?;

            if let Some(Err(e)) = transfer_result {
                return Err(map_curl_error(&e.to_string()));
            }

            Ok(easy)
        })
    }
}

impl FtpTransport for CurlTransport {
    fn list(&self, dir_url: &str) -> Result<String, TransportError> {
        let mut easy = Easy2::new(DownloadHandler::default());
        easy.url(dir_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        easy.custom_request("LIST")
            .map_err(|e| TransportError::Io(e.to_string()))?;

        let easy = self.run_transfer(easy)?;
        Ok(String::from_utf8_lossy(&easy.get_ref().out).to_string())
    }

    fn read_link(&self, path_url: &str) -> Result<String, TransportError> {
        let mut easy = Easy2::new(DownloadHandler::default());
        easy.url(path_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;

        let easy = self.run_transfer(easy)?;
        Ok(String::from_utf8_lossy(&easy.get_ref().out)
            .trim()
            .to_string())
    }

    fn read_file_range(
        &self,
        path_url: &str,
        offset: u64,
        size: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let mut easy = Easy2::new(DownloadHandler::default());
        easy.url(path_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        let end = offset + size.saturating_sub(1) as u64;
        easy.range(&format!("{offset}-{end}"))
            .map_err(|e| TransportError::Io(e.to_string()))?;

        let easy = self.run_transfer(easy)?;
        Ok(easy.get_ref().out.clone())
    }

    fn write_file(&self, path_url: &str, data: &[u8]) -> Result<(), TransportError> {
        let mut easy = Easy2::new(UploadHandler {
            data: data.to_vec(),
            pos: 0,
            out: Vec::new(),
        });

        easy.url(path_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        easy.upload(true)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        easy.in_filesize(data.len() as u64)
            .map_err(|e| TransportError::Io(e.to_string()))?;

        self.run_transfer(easy)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct DownloadHandler {
    out: Vec<u8>,
}

impl Handler for DownloadHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.out.extend_from_slice(data);
        Ok(data.len())
    }
}

#[derive(Debug, Default)]
struct UploadHandler {
    data: Vec<u8>,
    pos: usize,
    out: Vec<u8>,
}

impl Handler for UploadHandler {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ReadError> {
        let remaining = self.data.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }
        let n = remaining.min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.out.extend_from_slice(data);
        Ok(data.len())
    }
}

fn map_curl_error(message: &str) -> TransportError {
    if message.contains("404") || message.contains("No such file") {
        TransportError::NotFound
    } else {
        TransportError::Io(message.to_string())
    }
}
