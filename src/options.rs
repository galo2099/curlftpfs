use crate::remote::{CurlConfig, SslMode};
use fuser::MountOption;
use std::{ffi::OsString, path::PathBuf, time::Duration};

pub const HELP: &str = r#"usage: curlftpfs [options] <host> <mountpoint>

Mount an FTP server through FUSE and libcurl.

General options:
    -h, --help                 print help
    -V, --version              print version
    -v, --verbose              make libcurl verbose
    -o opt[,opt...]            FUSE/curlftpfs mount options

curlftpfs options (usable after -o):
    user=USER[:PASS]           FTP credentials
    connect_timeout=SECONDS    connection timeout
    disable_epsv | enable_epsv control EPSV
    skip_pasv_ip               ignore the address in PASV replies
    ftp_port=ADDR              use active FTP from ADDR
    disable_eprt               disable EPRT
    ftp_method=METHOD          multicwd, nocwd, or singlecwd
    custom_list=COMMAND        directory listing command (default: MLSD)
    tcp_nodelay                enable TCP_NODELAY
    ssl | ssl_control | ssl_try enable FTPS
    no_verify_hostname         disable TLS hostname verification
    no_verify_peer             disable TLS peer verification
    cert=FILE, cert_type=TYPE, key=FILE, key_type=TYPE, pass=PASS
    cacert=FILE, capath=DIR, ciphers=LIST
    interface=NAME             outgoing network interface
    proxy=URL, proxy_user=U:P  proxy settings
    proxytunnel                tunnel through HTTP proxy
    httpproxy | socks4 | socks5 select proxy kind
    ipv4 | ipv6               address family
    cache_timeout=SECONDS      metadata cache lifetime (default: 10)

Standard FUSE options such as ro, rw, allow_other, default_permissions,
auto_unmount, nodev, noexec, nosuid, sync, dirsync, and fsname=NAME are passed on.
"#;

pub enum ParseResult {
    Run(Box<Args>),
    Help,
    Version,
}
pub struct Args {
    pub url: String,
    pub mountpoint: PathBuf,
    pub curl: CurlConfig,
    pub mount_options: Vec<MountOption>,
    pub cache_timeout: Duration,
}

impl Args {
    pub fn parse(argv: &[OsString]) -> Result<ParseResult, String> {
        let mut cfg = CurlConfig::default();
        let mut fuse = Vec::new();
        let mut positional = Vec::new();
        let mut timeout = Duration::from_secs(10);
        let mut i = 1;
        while i < argv.len() {
            let arg = argv[i].to_string_lossy();
            match arg.as_ref() {
                "-h" | "--help" => return Ok(ParseResult::Help),
                "-V" | "--version" => return Ok(ParseResult::Version),
                "-v" | "--verbose" => cfg.verbose = true,
                "-f" => fuse.push(MountOption::CUSTOM("-f".into())),
                "-s" => fuse.push(MountOption::CUSTOM("-s".into())),
                "-o" => {
                    i += 1;
                    let value = argv
                        .get(i)
                        .ok_or("-o requires an argument")?
                        .to_string_lossy();
                    for o in value.split(',').filter(|x| !x.is_empty()) {
                        parse_option(o, &mut cfg, &mut fuse, &mut timeout)?;
                    }
                }
                x if x.starts_with("-o") => {
                    for o in x[2..].split(',').filter(|x| !x.is_empty()) {
                        parse_option(o, &mut cfg, &mut fuse, &mut timeout)?;
                    }
                }
                x if x.starts_with('-') => fuse.push(MountOption::CUSTOM(x.into())),
                _ => positional.push(arg.into_owned()),
            }
            i += 1;
        }
        if positional.len() != 2 {
            return Err("exactly one host and one mountpoint are required".into());
        }
        let mut url = positional.remove(0);
        if !url.contains("://") {
            url = format!("ftp://{url}");
        }
        if !url.ends_with('/') {
            url.push('/');
        }
        Ok(ParseResult::Run(Box::new(Args {
            url,
            mountpoint: positional.remove(0).into(),
            curl: cfg,
            mount_options: fuse,
            cache_timeout: timeout,
        })))
    }
}

fn parse_option(
    o: &str,
    c: &mut CurlConfig,
    fuse: &mut Vec<MountOption>,
    cache: &mut Duration,
) -> Result<(), String> {
    let (key, value) = o.split_once('=').map_or((o, None), |(k, v)| (k, Some(v)));
    macro_rules! val {
        () => {
            value.ok_or_else(|| format!("option {key} requires a value"))?
        };
    }
    match key {
        "user" => c.user = Some(val!().into()),
        "connect_timeout" => {
            c.connect_timeout = val!().parse().map_err(|_| "invalid connect_timeout")?
        }
        "disable_epsv" => c.epsv = false,
        "enable_epsv" => c.epsv = true,
        "skip_pasv_ip" => c.skip_pasv_ip = true,
        "ftp_port" => c.ftp_port = Some(val!().into()),
        "disable_eprt" => c.eprt = false,
        "ftp_method" => c.ftp_method = Some(val!().into()),
        "custom_list" => c.custom_list = Some(val!().into()),
        "tcp_nodelay" => c.tcp_nodelay = true,
        "ssl" => c.ssl = SslMode::All,
        "ssl_control" => c.ssl = SslMode::Control,
        "ssl_try" => c.ssl = SslMode::Try,
        "no_verify_hostname" => c.verify_host = false,
        "no_verify_peer" => c.verify_peer = false,
        "cert" => c.cert = Some(val!().into()),
        "cert_type" => c.cert_type = Some(val!().into()),
        "key" => c.key = Some(val!().into()),
        "key_type" => c.key_type = Some(val!().into()),
        "pass" => c.key_password = Some(val!().into()),
        "cacert" => c.cacert = Some(val!().into()),
        "capath" => c.capath = Some(val!().into()),
        "ciphers" => c.ciphers = Some(val!().into()),
        "interface" => c.interface = Some(val!().into()),
        "proxy" => c.proxy = Some(val!().into()),
        "proxy_user" => c.proxy_user = Some(val!().into()),
        "proxytunnel" => c.proxy_tunnel = true,
        "httpproxy" => c.proxy_type = curl::easy::ProxyType::Http,
        "socks4" => c.proxy_type = curl::easy::ProxyType::Socks4,
        "socks5" => c.proxy_type = curl::easy::ProxyType::Socks5,
        "ipv4" => c.ip = curl::easy::IpResolve::V4,
        "ipv6" => c.ip = curl::easy::IpResolve::V6,
        "cache_timeout" => {
            *cache = Duration::from_secs(val!().parse().map_err(|_| "invalid cache_timeout")?)
        }
        "ro" => fuse.push(MountOption::RO),
        "rw" => fuse.push(MountOption::RW),
        "allow_other" => fuse.push(MountOption::AllowOther),
        "allow_root" => fuse.push(MountOption::AllowRoot),
        "auto_unmount" => fuse.push(MountOption::AutoUnmount),
        "default_permissions" => fuse.push(MountOption::DefaultPermissions),
        "nodev" => fuse.push(MountOption::NoDev),
        "noexec" => fuse.push(MountOption::NoExec),
        "nosuid" => fuse.push(MountOption::NoSuid),
        "sync" => fuse.push(MountOption::Sync),
        "async" => fuse.push(MountOption::Async),
        "dirsync" => fuse.push(MountOption::DirSync),
        "atime" => fuse.push(MountOption::Atime),
        "noatime" => fuse.push(MountOption::NoAtime),
        "relatime" => fuse.push(MountOption::CUSTOM("relatime".into())),
        "transform_symlinks" | "tryutf8" | "utf8" | "nomulticonn" | "ftpfs_debug" | "codepage"
        | "iocharset" => log::warn!("legacy option {key} is accepted but no longer necessary"),
        "fsname" => fuse.push(MountOption::FSName(val!().into())),
        "subtype" => fuse.push(MountOption::Subtype(val!().into())),
        _ => fuse.push(MountOption::CUSTOM(o.into())),
    }
    Ok(())
}
