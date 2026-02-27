use curlftpfs::rustfs::FtpFuseFs;

#[test]
fn ftp_url_validation_happens() {
    assert!(FtpFuseFs::new("not-a-url").is_err());
    assert!(FtpFuseFs::new("ftp://example.com/").is_ok());
}

#[test]
fn mountpoint_validation_happens() {
    let fs = FtpFuseFs::new("ftp://example.com/").expect("fs");
    let err = fs
        .mount("/definitely/not/a/real/mountpoint")
        .expect_err("mount should fail");
    assert!(err.contains("mountpoint does not exist"));
}
