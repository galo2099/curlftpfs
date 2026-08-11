use curl::easy::{Easy, IpResolve, ProxyType};
use std::{io, path::Path, sync::Mutex, time::Duration};

#[derive(Clone, Copy, Default)]
pub enum SslMode {
    #[default]
    Off,
    Try,
    Control,
    All,
}
#[derive(Clone)]
pub struct CurlConfig {
    pub verbose: bool,
    pub connect_timeout: u64,
    pub epsv: bool,
    pub eprt: bool,
    pub skip_pasv_ip: bool,
    pub tcp_nodelay: bool,
    pub user: Option<String>,
    pub ftp_port: Option<String>,
    pub ftp_method: Option<String>,
    pub custom_list: Option<String>,
    pub ssl: SslMode,
    pub verify_host: bool,
    pub verify_peer: bool,
    pub cert: Option<String>,
    pub cert_type: Option<String>,
    pub key: Option<String>,
    pub key_type: Option<String>,
    pub key_password: Option<String>,
    pub cacert: Option<String>,
    pub capath: Option<String>,
    pub ciphers: Option<String>,
    pub interface: Option<String>,
    pub proxy: Option<String>,
    pub proxy_user: Option<String>,
    pub proxy_tunnel: bool,
    pub proxy_type: ProxyType,
    pub ip: IpResolve,
}
impl Default for CurlConfig {
    fn default() -> Self {
        Self {
            verbose: false,
            connect_timeout: 0,
            epsv: true,
            eprt: true,
            skip_pasv_ip: false,
            tcp_nodelay: false,
            user: None,
            ftp_port: None,
            ftp_method: None,
            custom_list: None,
            ssl: SslMode::Off,
            verify_host: true,
            verify_peer: true,
            cert: None,
            cert_type: None,
            key: None,
            key_type: None,
            key_password: None,
            cacert: None,
            capath: None,
            ciphers: None,
            interface: None,
            proxy: None,
            proxy_user: None,
            proxy_tunnel: false,
            proxy_type: ProxyType::Http,
            ip: IpResolve::Any,
        }
    }
}

pub struct Remote {
    base: String,
    cfg: CurlConfig,
    lock: Mutex<()>,
}
impl Remote {
    pub fn new(base: String, cfg: CurlConfig) -> Result<Self, String> {
        curl::init();
        Ok(Self {
            base,
            cfg,
            lock: Mutex::new(()),
        })
    }
    fn url(&self, path: &Path) -> String {
        let p = path
            .to_string_lossy()
            .trim_start_matches('/')
            .split('/')
            .map(url_encode)
            .collect::<Vec<_>>()
            .join("/");
        format!("{}{p}", self.base)
    }
    fn easy(&self, path: &Path) -> io::Result<Easy> {
        let mut e = Easy::new();
        e.url(&self.url(path)).map_err(err)?;
        e.verbose(self.cfg.verbose).map_err(err)?;
        if self.cfg.connect_timeout > 0 {
            e.connect_timeout(Duration::from_secs(self.cfg.connect_timeout))
                .map_err(err)?;
        }
        e.tcp_nodelay(self.cfg.tcp_nodelay).map_err(err)?;
        e.ssl_verify_host(self.cfg.verify_host).map_err(err)?;
        e.ssl_verify_peer(self.cfg.verify_peer).map_err(err)?;
        e.ip_resolve(self.cfg.ip).map_err(err)?;
        if let Some(v) = &self.cfg.user {
            let (u, p) = v.split_once(':').unwrap_or((v, ""));
            e.username(u).map_err(err)?;
            e.password(p).map_err(err)?;
        }
        if let Some(v) = &self.cfg.interface {
            e.interface(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.proxy {
            e.proxy(v).map_err(err)?;
            e.proxy_type(self.cfg.proxy_type).map_err(err)?;
        }
        if let Some(v) = &self.cfg.proxy_user {
            let (u, p) = v.split_once(':').unwrap_or((v, ""));
            e.proxy_username(u).map_err(err)?;
            e.proxy_password(p).map_err(err)?;
        }
        e.http_proxy_tunnel(self.cfg.proxy_tunnel).map_err(err)?;
        if let Some(v) = &self.cfg.cert {
            e.ssl_cert(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.cert_type {
            e.ssl_cert_type(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.key {
            e.ssl_key(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.key_type {
            e.ssl_key_type(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.key_password {
            e.key_password(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.cacert {
            e.cainfo(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.capath {
            e.capath(v).map_err(err)?;
        }
        if let Some(v) = &self.cfg.ciphers {
            e.ssl_cipher_list(v).map_err(err)?;
        }
        Ok(e)
    }
    pub fn download(&self, path: &Path) -> io::Result<Vec<u8>> {
        let _g = self.lock.lock().unwrap();
        let mut e = self.easy(path)?;
        let mut out = Vec::new();
        {
            let mut t = e.transfer();
            t.write_function(|d| {
                out.extend_from_slice(d);
                Ok(d.len())
            })
            .map_err(err)?;
            t.perform().map_err(err)?;
        }
        Ok(out)
    }
    pub fn upload(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut e = self.easy(path)?;
        e.upload(true).map_err(err)?;
        e.in_filesize(data.len() as u64).map_err(err)?;
        let mut pos = 0;
        {
            let mut t = e.transfer();
            t.read_function(|b| {
                let n = (data.len() - pos).min(b.len());
                b[..n].copy_from_slice(&data[pos..pos + n]);
                pos += n;
                Ok(n)
            })
            .map_err(err)?;
            t.perform().map_err(err)?;
        }
        Ok(())
    }
    pub fn list(&self, path: &Path) -> io::Result<Vec<ListEntry>> {
        let _g = self.lock.lock().unwrap();
        let mut e = self.easy(path)?;
        e.custom_request(self.cfg.custom_list.as_deref().unwrap_or("MLSD"))
            .map_err(err)?;
        let mut out = Vec::new();
        {
            let mut t = e.transfer();
            t.write_function(|d| {
                out.extend_from_slice(d);
                Ok(d.len())
            })
            .map_err(err)?;
            t.perform().map_err(err)?;
        }
        Ok(parse_listing(&String::from_utf8_lossy(&out)))
    }
    pub fn command(&self, path: &Path, command: String) -> io::Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut e = self.easy(path)?;
        e.nobody(true).map_err(err)?;
        let mut list = std::ptr::null_mut();
        for command in command.lines() {
            let command = std::ffi::CString::new(command)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in FTP command"))?;
            list = unsafe { curl_sys::curl_slist_append(list, command.as_ptr()) };
        }
        // The safe curl wrapper does not expose CURLOPT_QUOTE. libcurl copies
        // each command, and the list remains alive until perform completes.
        let result = unsafe { curl_sys::curl_easy_setopt(e.raw(), curl_sys::CURLOPT_QUOTE, list) };
        let performed = if result == curl_sys::CURLE_OK {
            e.perform().map_err(err)
        } else {
            Err(io::Error::other(format!("libcurl setopt failed: {result}")))
        };
        unsafe { curl_sys::curl_slist_free_all(list) };
        performed
    }
}
#[derive(Clone, Debug)]
pub struct ListEntry {
    pub name: String,
    pub size: u64,
    pub directory: bool,
    pub symlink: bool,
    pub modified: i64,
}
fn parse_listing(s: &str) -> Vec<ListEntry> {
    s.lines()
        .filter_map(|line| {
            let line = line.trim_matches(['\r', '\n']);
            let (facts, name) = line.split_once(' ')?;
            let mut x = ListEntry {
                name: name.into(),
                size: 0,
                directory: false,
                symlink: false,
                modified: 0,
            };
            for f in facts.split(';') {
                let Some((k, v)) = f.split_once('=') else {
                    continue;
                };
                match k.to_ascii_lowercase().as_str() {
                    "type" => {
                        x.directory = v.eq_ignore_ascii_case("dir")
                            || v.eq_ignore_ascii_case("cdir")
                            || v.eq_ignore_ascii_case("pdir");
                        x.symlink = v.to_ascii_lowercase().contains("slink")
                    }
                    "size" => x.size = v.parse().unwrap_or(0),
                    _ => {}
                }
            }
            if name == "." || name == ".." {
                None
            } else {
                Some(x)
            }
        })
        .collect()
}
fn url_encode(s: &str) -> String {
    s.bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                vec![b as char]
            } else {
                format!("%{b:02X}").chars().collect()
            }
        })
        .collect()
}
fn err(e: curl::Error) -> io::Error {
    let code = if e.is_couldnt_connect() {
        io::ErrorKind::ConnectionRefused
    } else if e.code() == 78 {
        io::ErrorKind::NotFound
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(code, e)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_mlsd() {
        let v = parse_listing("type=file;size=12; a.txt\r\ntype=dir; sub\r\n");
        assert_eq!(v[0].name, "a.txt");
        assert_eq!(v[0].size, 12);
        assert!(v[1].directory);
    }
    #[test]
    fn escapes_paths() {
        assert_eq!(url_encode("a b"), "a%20b");
    }
}
