use chrono::{Datelike, Local, NaiveDateTime, TimeZone};
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
    pub proxy_auth: u64,
    pub ssl_version: Option<i64>,
    pub engine: Option<String>,
    pub krb4: Option<String>,
    pub codepage: Option<String>,
    pub iocharset: Option<String>,
    pub transform_symlinks: bool,
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
            proxy_auth: 0,
            ssl_version: None,
            engine: None,
            krb4: None,
            codepage: None,
            iocharset: None,
            transform_symlinks: false,
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
        setopt_long(&e, curl_sys::CURLOPT_FTP_USE_EPSV, self.cfg.epsv as i64)?;
        setopt_long(&e, curl_sys::CURLOPT_FTP_USE_EPRT, self.cfg.eprt as i64)?;
        setopt_long(
            &e,
            curl_sys::CURLOPT_FTP_SKIP_PASV_IP,
            self.cfg.skip_pasv_ip as i64,
        )?;
        setopt_long(
            &e,
            curl_sys::CURLOPT_USE_SSL,
            match self.cfg.ssl {
                SslMode::Off => 0,
                SslMode::Try => 1,
                SslMode::Control => 2,
                SslMode::All => 3,
            },
        )?;
        if let Some(method) = &self.cfg.ftp_method {
            let method = match method.as_str() {
                "multicwd" => 1,
                "nocwd" => 2,
                "singlecwd" => 3,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "ftp_method must be multicwd, nocwd, or singlecwd",
                    ))
                }
            };
            setopt_long(&e, curl_sys::CURLOPT_FTP_FILEMETHOD, method)?;
        }
        if let Some(version) = self.cfg.ssl_version {
            setopt_long(&e, curl_sys::CURLOPT_SSLVERSION, version)?;
        }
        if self.cfg.proxy_auth != 0 {
            setopt_long(&e, curl_sys::CURLOPT_PROXYAUTH, self.cfg.proxy_auth as i64)?;
        }
        if let Some(v) = &self.cfg.ftp_port {
            setopt_string(&e, curl_sys::CURLOPT_FTPPORT, v)?;
        }
        if let Some(v) = &self.cfg.engine {
            setopt_string(&e, curl_sys::CURLOPT_SSLENGINE, v)?;
        }
        if let Some(v) = &self.cfg.krb4 {
            setopt_string(&e, curl_sys::CURLOPT_KRBLEVEL, v)?;
        }
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
        if let Some(command) = &self.cfg.custom_list {
            return self.list_command(path, command);
        }
        // MLSD is machine-readable but is not implemented by many older FTP
        // servers. Preserve curlftpfs compatibility by retrying with LIST.
        self.list_command(path, "MLSD")
            .or_else(|_| self.list_command(path, "LIST"))
    }

    fn list_command(&self, path: &Path, command: &str) -> io::Result<Vec<ListEntry>> {
        let mut e = self.easy(path)?;
        e.custom_request(command).map_err(err)?;
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
        let listing = if let Some(label) = &self.cfg.codepage {
            let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown codepage {label}"),
                )
            })?;
            if self.cfg.iocharset.as_deref().is_some_and(|charset| {
                !charset.eq_ignore_ascii_case("UTF-8") && !charset.eq_ignore_ascii_case("UTF8")
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only UTF-8 iocharset is supported by the Rust path API",
                ));
            }
            encoding.decode(&out).0
        } else {
            String::from_utf8_lossy(&out)
        };
        let parsed = parse_listing(&listing);
        if parsed.is_empty() && !out.is_empty() {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported {command} directory listing"),
            ))
        } else {
            Ok(parsed)
        }
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
    pub link_target: Option<String>,
    pub perm: u16,
    pub nlink: u32,
}
fn parse_listing(s: &str) -> Vec<ListEntry> {
    s.lines().filter_map(parse_listing_line).collect()
}

fn parse_listing_line(line: &str) -> Option<ListEntry> {
    let line = line.trim_matches(['\r', '\n']);
    parse_mlsd(line)
        .or_else(|| parse_dir_unix(line))
        .or_else(|| parse_dir_windows(line))
}

fn parse_mlsd(line: &str) -> Option<ListEntry> {
    let (facts, name) = line.split_once(' ')?;
    if !facts.contains('=') {
        return None;
    }
    let mut x = ListEntry {
        name: name.into(),
        size: 0,
        directory: false,
        symlink: false,
        modified: 0,
        link_target: None,
        perm: 0,
        nlink: 1,
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
                x.symlink = v.to_ascii_lowercase().contains("slink");
                if let Some((_, target)) = v.split_once(':') {
                    x.link_target = Some(target.to_owned());
                }
            }
            "size" => x.size = v.parse().unwrap_or(0),
            "unix.mode" => x.perm = u16::from_str_radix(v, 8).unwrap_or(0) & 0o7777,
            "modify" => {
                x.modified = NaiveDateTime::parse_from_str(
                    v.trim_end_matches(|c: char| !c.is_ascii_digit()),
                    "%Y%m%d%H%M%S",
                )
                .map(|date| date.and_utc().timestamp())
                .unwrap_or(0)
            }
            _ => {}
        }
    }
    (name != "." && name != "..").then_some(x)
}

/// Parse the traditional `LIST` format emitted by Unix FTP servers.
///
/// This is the Rust equivalent of the old `parse_dir_unix`: it accepts both
/// listings with and without a link-count column and retains names containing
/// whitespace. Symlink targets are removed from the displayed filename.
fn parse_dir_unix(line: &str) -> Option<ListEntry> {
    let fields: Vec<(usize, &str)> = line
        .match_indices(|c: char| !c.is_whitespace())
        .filter(|(i, _)| {
            *i == 0
                || line[..*i]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace)
        })
        .map(|(start, _)| {
            let end = line[start..]
                .find(char::is_whitespace)
                .map_or(line.len(), |n| start + n);
            (start, &line[start..end])
        })
        .collect();
    let mode = fields.first()?.1;
    if mode.len() < 10 || !matches!(mode.as_bytes()[0], b'-' | b'd' | b'l') {
        return None;
    }
    let parsed_nlink = fields.get(1)?.1.parse::<u32>().ok();
    let has_nlink = parsed_nlink.is_some();
    let size_index = if has_nlink { 4 } else { 3 };
    let name_index = if has_nlink { 8 } else { 7 };
    let size = fields.get(size_index)?.1.parse().ok()?;
    let month = fields.get(size_index + 1)?.1;
    let day = fields.get(size_index + 2)?.1;
    let year_or_time = fields.get(size_index + 3)?.1;
    let name_start = fields.get(name_index)?.0;
    let mut name = line[name_start..].to_owned();
    let symlink = mode.starts_with('l');
    let mut link_target = None;
    if symlink {
        if let Some((file, _target)) = name.split_once(" -> ") {
            link_target = name.split_once(" -> ").map(|(_, target)| target.to_owned());
            name = file.to_owned();
        }
    }
    Some(ListEntry {
        name,
        size,
        directory: mode.starts_with('d'),
        symlink,
        modified: parse_unix_date(month, day, year_or_time),
        link_target,
        perm: mode.as_bytes()[1..10]
            .iter()
            .enumerate()
            .fold(0, |bits, (i, c)| {
                if *c != b'-' {
                    bits | 1 << (8 - i)
                } else {
                    bits
                }
            }),
        nlink: parsed_nlink.unwrap_or(1),
    })
}

fn parse_dir_windows(line: &str) -> Option<ListEntry> {
    let mut fields = line.split_whitespace();
    let date = fields.next()?;
    let time = fields.next()?;
    let size_or_dir = fields.next()?;
    if !date.contains('-') || !(time.ends_with("AM") || time.ends_with("PM")) {
        return None;
    }
    let name = fields.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }
    let directory = size_or_dir.eq_ignore_ascii_case("<DIR>");
    let size = if directory {
        0
    } else {
        size_or_dir.parse().ok()?
    };
    Some(ListEntry {
        name,
        size,
        directory,
        symlink: false,
        modified: parse_windows_date(date, time),
        link_target: None,
        perm: if directory { 0o755 } else { 0o644 },
        nlink: 1,
    })
}

fn parse_unix_date(month: &str, day: &str, year_or_time: &str) -> i64 {
    let now = Local::now();
    let text = if year_or_time.contains(':') {
        format!("{} {month} {day} {year_or_time}", now.year())
    } else {
        format!("{year_or_time} {month} {day} 00:00")
    };
    let Ok(mut date) = NaiveDateTime::parse_from_str(&text, "%Y %b %d %H:%M") else {
        return 0;
    };
    if year_or_time.contains(':') && date > now.naive_local() + chrono::Duration::days(1) {
        date = date.with_year(date.year() - 1).unwrap_or(date);
    }
    Local
        .from_local_datetime(&date)
        .single()
        .map_or(0, |date| date.timestamp())
}

fn parse_windows_date(date: &str, time: &str) -> i64 {
    NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%m-%d-%y %I:%M%p")
        .ok()
        .and_then(|date| Local.from_local_datetime(&date).single())
        .map_or(0, |date| date.timestamp())
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

fn setopt_long(e: &Easy, option: curl_sys::CURLoption, value: i64) -> io::Result<()> {
    let result = unsafe { curl_sys::curl_easy_setopt(e.raw(), option, value as libc::c_long) };
    (result == curl_sys::CURLE_OK)
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("libcurl setopt {option} failed: {result}")))
}

fn setopt_string(e: &Easy, option: curl_sys::CURLoption, value: &str) -> io::Result<()> {
    let value = std::ffi::CString::new(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in curl option"))?;
    let result = unsafe { curl_sys::curl_easy_setopt(e.raw(), option, value.as_ptr()) };
    (result == curl_sys::CURLE_OK)
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("libcurl setopt {option} failed: {result}")))
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
    fn parses_unix_list_with_spaces_and_symlinks() {
        let v = parse_listing("-rw-r--r-- 1 alice users 12 Jan 02 2025 file with spaces.txt\r\nlrwxrwxrwx 1 root root 4 Feb 03 12:30 link -> dest\r\n");
        assert_eq!(v[0].name, "file with spaces.txt");
        assert_eq!(v[0].size, 12);
        assert_eq!(v[1].name, "link");
        assert!(v[1].symlink);
        assert_eq!(v[1].link_target.as_deref(), Some("dest"));
        assert_eq!(v[0].perm, 0o644);
        assert_eq!(v[0].nlink, 1);
        assert!(v[0].modified > 0);
    }
    #[test]
    fn parses_unix_list_without_link_count() {
        let v = parse_listing("drwxr-xr-x owner group 4096 Mar 04 2024 directory\n");
        assert_eq!(v[0].name, "directory");
        assert!(v[0].directory);
        assert!(v[0].modified > 0);
    }
    #[test]
    fn parses_windows_list() {
        let v = parse_listing("01-02-24  03:04PM       <DIR>          My Folder\r\n");
        assert_eq!(v[0].name, "My Folder");
        assert!(v[0].directory);
    }
    #[test]
    fn escapes_paths() {
        assert_eq!(url_encode("a b"), "a%20b");
    }
}
