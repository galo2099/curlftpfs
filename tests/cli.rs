use std::fs;
use std::path::Path;

use curlftpfs::cli::{is_self_delegate, parse_args, CliMode};

#[test]
fn parse_mode_parses_listing_file() {
    let args = vec![
        "curlftpfs".to_string(),
        "parse".to_string(),
        "list.txt".to_string(),
    ];
    let mode = parse_args(&args).expect("parse args");
    assert_eq!(
        mode,
        CliMode::Parse {
            listing_file: "list.txt".into()
        }
    );
}

#[test]
fn mount_mode_parses_site_mountpoint_and_options() {
    let args = vec![
        "curlftpfs".to_string(),
        "mount".to_string(),
        "ftp://example".to_string(),
        "/mnt/x".to_string(),
        "-o".to_string(),
        "ro".to_string(),
    ];

    let mode = parse_args(&args).expect("parse args");
    assert_eq!(
        mode,
        CliMode::Mount {
            ftp_site: "ftp://example".to_string(),
            mountpoint: "/mnt/x".to_string(),
            extra_options: vec!["-o".to_string(), "ro".to_string()],
        }
    );
}

#[test]
fn self_delegate_detected_for_same_real_path() {
    let mut p = std::env::temp_dir();
    p.push(format!("curlftpfs-cli-test-{}", std::process::id()));
    fs::write(&p, b"x").expect("write temp");
    assert!(is_self_delegate(Path::new(&p), Path::new(&p)));
    fs::remove_file(&p).ok();
}
