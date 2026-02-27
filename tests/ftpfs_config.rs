use curlftpfs::ftpfs_config::parse_mount_options;

#[test]
fn parses_core_legacy_flags() {
    let args = vec![
        "ftpfs_debug=3".to_string(),
        "transform_symlinks".to_string(),
        "disable_epsv".to_string(),
        "enable_epsv".to_string(),
        "ftp_port=2121".to_string(),
        "ftp_method=multicwd".to_string(),
        "custom_list=LIST -la".to_string(),
        "connect_timeout=15".to_string(),
        "ssl".to_string(),
        "no_verify_peer".to_string(),
        "proxy=socks://proxy".to_string(),
        "proxy_ntlm".to_string(),
        "socks5".to_string(),
        "user=u:p".to_string(),
        "proxy_user=pu:pp".to_string(),
        "ipv6".to_string(),
        "utf8".to_string(),
        "codepage=CP1251".to_string(),
        "iocharset=UTF-8".to_string(),
        "nomulticonn".to_string(),
    ];

    let cfg = parse_mount_options(&args);
    assert_eq!(cfg.debug, 3);
    assert!(cfg.transform_symlinks);
    assert!(!cfg.disable_epsv);
    assert_eq!(cfg.ftp_port.as_deref(), Some("2121"));
    assert_eq!(cfg.ftp_method.as_deref(), Some("multicwd"));
    assert_eq!(cfg.custom_list.as_deref(), Some("LIST -la"));
    assert_eq!(cfg.connect_timeout, 15);
    assert_eq!(cfg.use_ssl, 3);
    assert!(cfg.no_verify_peer);
    assert_eq!(cfg.proxy.as_deref(), Some("socks://proxy"));
    assert!(cfg.proxy_ntlm);
    assert_eq!(cfg.proxy_type.as_deref(), Some("socks5"));
    assert_eq!(cfg.user.as_deref(), Some("u:p"));
    assert_eq!(cfg.proxy_user.as_deref(), Some("pu:pp"));
    assert_eq!(cfg.ip_version.as_deref(), Some("ipv6"));
    assert!(cfg.tryutf8);
    assert_eq!(cfg.codepage.as_deref(), Some("CP1251"));
    assert_eq!(cfg.iocharset.as_deref(), Some("UTF-8"));
    assert!(!cfg.multiconn);
}

#[test]
fn ssl_and_proxy_variants_overwrite_like_last_option_wins() {
    let args = vec![
        "ssl_try".to_string(),
        "ssl_control".to_string(),
        "tlsv1".to_string(),
        "sslv3".to_string(),
        "httpproxy".to_string(),
        "socks4".to_string(),
    ];

    let cfg = parse_mount_options(&args);
    assert_eq!(cfg.use_ssl, 2);
    assert_eq!(cfg.ssl_version.as_deref(), Some("sslv3"));
    assert_eq!(cfg.proxy_type.as_deref(), Some("socks4"));
}
