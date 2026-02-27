use std::sync::{Arc, Mutex};

use curlftpfs::cache::Cache;
use curlftpfs::fuse_ops::{FuseAdapter, FuseLikeOps};
use curlftpfs::parser::FtpFsConfig;
use curlftpfs::path_utils::PathContext;
use curlftpfs::service::FtpFsService;
use curlftpfs::transport::MemoryTransport;

fn make_service() -> FtpFsService {
    let mut tr = MemoryTransport::default();
    tr.listings.insert(
        "ftp://example/dir//".to_string(),
        "-rw-r--r--  1 user group 5 Jan 01  2001 file.txt\r\nlrwxrwxrwx  1 user group 4 Jan 01  2001 lnk -> /dst\r\n".to_string(),
    );
    tr.links
        .insert("ftp://example/dir/lnk".to_string(), "/dst".to_string());
    tr.files.insert(
        "ftp://example/dir/file.txt".to_string(),
        b"hello world".to_vec(),
    );

    FtpFsService::new(
        Arc::new(Mutex::new(tr)),
        Cache::default(),
        FtpFsConfig::default(),
        PathContext {
            host: "ftp://example/".to_string(),
            codepage: None,
            iocharset: None,
        },
    )
}

#[test]
fn fuse_lifecycle_and_readdir_getattr_readlink_read_and_write_path() {
    let service = make_service();
    let fuse = FuseAdapter::new(service);

    fuse.init();
    let entries = fuse.readdir("/dir/").expect("readdir");
    assert_eq!(entries.len(), 2);

    let st = fuse.getattr("/dir/file.txt").expect("getattr");
    assert_eq!(st.size, 5);

    let link = fuse.readlink("/dir/lnk").expect("readlink");
    assert_eq!(link, "/dst");

    let bytes = fuse.read("/dir/file.txt", 6, 5).expect("read");
    assert_eq!(bytes, b"world");

    fuse.open_write("/dir/new.txt").expect("open_write");
    assert_eq!(fuse.write("/dir/new.txt", 0, b"abc").expect("write"), 3);
    fuse.flush("/dir/new.txt").expect("flush");
    fuse.release("/dir/new.txt").expect("release");

    let uploaded = fuse.read("/dir/new.txt", 0, 10).expect("uploaded read");
    assert_eq!(uploaded, b"abc");

    fuse.destroy();
}
