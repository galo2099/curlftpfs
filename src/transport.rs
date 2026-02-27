use std::collections::HashMap;

use curl::easy::Easy;

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

#[derive(Debug, Default, Clone)]
pub struct CurlTransport;

impl FtpTransport for CurlTransport {
    fn list(&self, dir_url: &str) -> Result<String, TransportError> {
        let mut easy = Easy::new();
        easy.url(dir_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;

        easy.custom_request("LIST")
            .map_err(|e| TransportError::Io(e.to_string()))?;

        let bytes = perform_bytes(&mut easy)?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    fn read_link(&self, path_url: &str) -> Result<String, TransportError> {
        let mut easy = Easy::new();
        easy.url(path_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        let bytes = perform_bytes(&mut easy)?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    }

    fn read_file_range(
        &self,
        path_url: &str,
        offset: u64,
        size: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let mut easy = Easy::new();
        easy.url(path_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        let end = offset + size.saturating_sub(1) as u64;
        easy.range(&format!("{offset}-{end}"))
            .map_err(|e| TransportError::Io(e.to_string()))?;
        perform_bytes(&mut easy)
    }

    fn write_file(&self, path_url: &str, data: &[u8]) -> Result<(), TransportError> {
        let mut easy = Easy::new();
        easy.url(path_url)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        easy.upload(true)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        easy.in_filesize(data.len() as u64)
            .map_err(|e| TransportError::Io(e.to_string()))?;

        let mut pos = 0usize;
        {
            let mut transfer = easy.transfer();
            transfer
                .read_function(|buf| {
                    let remaining = data.len().saturating_sub(pos);
                    if remaining == 0 {
                        return Ok(0);
                    }
                    let n = remaining.min(buf.len());
                    buf[..n].copy_from_slice(&data[pos..pos + n]);
                    pos += n;
                    Ok(n)
                })
                .map_err(|e| TransportError::Io(e.to_string()))?;
            transfer
                .perform()
                .map_err(|e| map_curl_error(&e.to_string()))?;
        }

        Ok(())
    }
}

fn perform_bytes(easy: &mut Easy) -> Result<Vec<u8>, TransportError> {
    let mut out = Vec::new();
    {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|data| {
                out.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(|e| TransportError::Io(e.to_string()))?;
        transfer
            .perform()
            .map_err(|e| map_curl_error(&e.to_string()))?;
    }
    Ok(out)
}

fn map_curl_error(message: &str) -> TransportError {
    if message.contains("404") || message.contains("No such file") {
        TransportError::NotFound
    } else {
        TransportError::Io(message.to_string())
    }
}
