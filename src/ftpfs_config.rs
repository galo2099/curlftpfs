#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpFsRuntimeConfig {
    pub debug: u32,
    pub transform_symlinks: bool,
    pub disable_epsv: bool,
    pub skip_pasv_ip: bool,
    pub ftp_port: Option<String>,
    pub disable_eprt: bool,
    pub ftp_method: Option<String>,
    pub custom_list: Option<String>,
    pub tcp_nodelay: bool,
    pub connect_timeout: u32,
    pub use_ssl: u32,
    pub no_verify_hostname: bool,
    pub no_verify_peer: bool,
    pub cert: Option<String>,
    pub cert_type: Option<String>,
    pub key: Option<String>,
    pub key_type: Option<String>,
    pub key_password: Option<String>,
    pub engine: Option<String>,
    pub cacert: Option<String>,
    pub capath: Option<String>,
    pub ciphers: Option<String>,
    pub interface: Option<String>,
    pub krb4: Option<String>,
    pub proxy: Option<String>,
    pub proxytunnel: bool,
    pub proxy_anyauth: bool,
    pub proxy_basic: bool,
    pub proxy_digest: bool,
    pub proxy_ntlm: bool,
    pub proxy_type: Option<String>,
    pub user: Option<String>,
    pub proxy_user: Option<String>,
    pub ssl_version: Option<String>,
    pub ip_version: Option<String>,
    pub tryutf8: bool,
    pub codepage: Option<String>,
    pub iocharset: Option<String>,
    pub multiconn: bool,
}

impl Default for FtpFsRuntimeConfig {
    fn default() -> Self {
        Self {
            debug: 0,
            transform_symlinks: false,
            disable_epsv: false,
            skip_pasv_ip: false,
            ftp_port: None,
            disable_eprt: false,
            ftp_method: None,
            custom_list: None,
            tcp_nodelay: false,
            connect_timeout: 0,
            use_ssl: 0,
            no_verify_hostname: false,
            no_verify_peer: false,
            cert: None,
            cert_type: None,
            key: None,
            key_type: None,
            key_password: None,
            engine: None,
            cacert: None,
            capath: None,
            ciphers: None,
            interface: None,
            krb4: None,
            proxy: None,
            proxytunnel: false,
            proxy_anyauth: false,
            proxy_basic: false,
            proxy_digest: false,
            proxy_ntlm: false,
            proxy_type: None,
            user: None,
            proxy_user: None,
            ssl_version: None,
            ip_version: None,
            tryutf8: false,
            codepage: None,
            iocharset: None,
            multiconn: true,
        }
    }
}

pub fn parse_mount_options(args: &[String]) -> FtpFsRuntimeConfig {
    let mut cfg = FtpFsRuntimeConfig::default();

    for arg in args {
        if let Some(v) = arg.strip_prefix("ftpfs_debug=") {
            if let Ok(d) = v.parse::<u32>() {
                cfg.debug = d;
            }
        } else if arg == "transform_symlinks" {
            cfg.transform_symlinks = true;
        } else if arg == "disable_epsv" {
            cfg.disable_epsv = true;
        } else if arg == "enable_epsv" {
            cfg.disable_epsv = false;
        } else if arg == "skip_pasv_ip" {
            cfg.skip_pasv_ip = true;
        } else if let Some(v) = arg.strip_prefix("ftp_port=") {
            cfg.ftp_port = Some(v.to_string());
        } else if arg == "disable_eprt" {
            cfg.disable_eprt = true;
        } else if let Some(v) = arg.strip_prefix("ftp_method=") {
            cfg.ftp_method = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("custom_list=") {
            cfg.custom_list = Some(v.to_string());
        } else if arg == "tcp_nodelay" {
            cfg.tcp_nodelay = true;
        } else if let Some(v) = arg.strip_prefix("connect_timeout=") {
            if let Ok(t) = v.parse::<u32>() {
                cfg.connect_timeout = t;
            }
        } else if arg == "ssl" {
            cfg.use_ssl = 3;
        } else if arg == "ssl_control" {
            cfg.use_ssl = 2;
        } else if arg == "ssl_try" {
            cfg.use_ssl = 1;
        } else if arg == "no_verify_hostname" {
            cfg.no_verify_hostname = true;
        } else if arg == "no_verify_peer" {
            cfg.no_verify_peer = true;
        } else if let Some(v) = arg.strip_prefix("cert=") {
            cfg.cert = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("cert_type=") {
            cfg.cert_type = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("key=") {
            cfg.key = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("key_type=") {
            cfg.key_type = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("pass=") {
            cfg.key_password = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("engine=") {
            cfg.engine = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("cacert=") {
            cfg.cacert = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("capath=") {
            cfg.capath = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("ciphers=") {
            cfg.ciphers = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("interface=") {
            cfg.interface = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("krb4=") {
            cfg.krb4 = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("proxy=") {
            cfg.proxy = Some(v.to_string());
        } else if arg == "proxytunnel" {
            cfg.proxytunnel = true;
        } else if arg == "proxy_anyauth" {
            cfg.proxy_anyauth = true;
        } else if arg == "proxy_basic" {
            cfg.proxy_basic = true;
        } else if arg == "proxy_digest" {
            cfg.proxy_digest = true;
        } else if arg == "proxy_ntlm" {
            cfg.proxy_ntlm = true;
        } else if arg == "httpproxy" {
            cfg.proxy_type = Some("http".to_string());
        } else if arg == "socks4" {
            cfg.proxy_type = Some("socks4".to_string());
        } else if arg == "socks5" {
            cfg.proxy_type = Some("socks5".to_string());
        } else if let Some(v) = arg.strip_prefix("user=") {
            cfg.user = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("proxy_user=") {
            cfg.proxy_user = Some(v.to_string());
        } else if arg == "tlsv1" {
            cfg.ssl_version = Some("tlsv1".to_string());
        } else if arg == "sslv3" {
            cfg.ssl_version = Some("sslv3".to_string());
        } else if arg == "ipv4" {
            cfg.ip_version = Some("ipv4".to_string());
        } else if arg == "ipv6" {
            cfg.ip_version = Some("ipv6".to_string());
        } else if arg == "utf8" {
            cfg.tryutf8 = true;
        } else if let Some(v) = arg.strip_prefix("codepage=") {
            cfg.codepage = Some(v.to_string());
        } else if let Some(v) = arg.strip_prefix("iocharset=") {
            cfg.iocharset = Some(v.to_string());
        } else if arg == "nomulticonn" {
            cfg.multiconn = false;
        }
    }

    cfg
}
