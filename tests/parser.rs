use curlftpfs::cache::{Cache, CacheLookup};
use curlftpfs::parser::{parse_dir, parse_dir_and_cache, parse_dir_for_name, FtpFsConfig};

#[test]
fn parses_windows_directory() {
    let cfg = FtpFsConfig::default();
    let list = "05-22-03  12:13PM       <DIR>          chinese_pr\r\n";
    let entries = parse_dir(list, "/", &cfg);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.name, "chinese_pr");
    assert_eq!(e.stat.mode, 0o040000);
    assert_eq!(e.stat.size, 0);
}

#[test]
fn parses_windows_file() {
    let cfg = FtpFsConfig::default();
    let list = "11-25-04  09:17AM             20075882 242_310_Condor_en_ok.pdf\r\n";
    let entries = parse_dir(list, "/", &cfg);
    let e = &entries[0];
    assert_eq!(e.name, "242_310_Condor_en_ok.pdf");
    assert_eq!(e.stat.mode, 0o100000);
    assert_eq!(e.stat.size, 20_075_882);
}

#[test]
fn parses_unix_symlink() {
    let cfg = FtpFsConfig::default();
    let list = "lrwxrwxrwx   1 1             17 Nov 24  2002 lg -> cidirb/documentos\r\n";
    let entries = parse_dir(list, "/", &cfg);
    let e = &entries[0];
    assert_eq!(e.name, "lg");
    assert_eq!(e.symlink_target.as_deref(), Some("cidirb/documentos"));
    assert_eq!(e.stat.mode, 0o120777);
}

#[test]
fn parses_unix_file_with_leading_space_name_like_legacy_c() {
    let cfg = FtpFsConfig::default();
    let list = "-rw-r--r--  1 robson users   1803128 Jan 01  2001  test\r\n";
    let entries = parse_dir(list, "/", &cfg);
    assert_eq!(entries[0].name, " test");
    assert_eq!(entries[0].stat.mode, 0o100644);
}

#[test]
fn find_by_name_and_root_dir_fallback() {
    let cfg = FtpFsConfig::default();
    let list = "drwxr-xr-x  4 robson users   4096 Mar 10  2020 tests\r\n";

    let found = parse_dir_for_name(list, "/", "tests", &cfg).expect("entry expected");
    assert_eq!(found.name, "tests");

    let root = parse_dir_for_name(list, "/", "", &cfg).expect("root expected");
    assert_eq!(root.stat.mode, 0o040755);
    assert_eq!(root.stat.size, 1024);
}

#[test]
fn parse_and_cache_populates_cache_entries() {
    let cfg = FtpFsConfig::default();
    let cache = Cache::default();
    let list = "lrwxrwxrwx   1 1             17 Nov 24  2002 lg -> cidirb/documentos\r\n";

    let entries = parse_dir_and_cache(list, "/", &cfg, &cache);
    assert_eq!(entries.len(), 1);

    assert!(matches!(cache.get_attr("/lg"), CacheLookup::Hit(_)));
    assert_eq!(
        cache.get_link("/lg"),
        CacheLookup::Hit("cidirb/documentos".to_string())
    );
    assert_eq!(cache.get_dir("/"), CacheLookup::Hit(vec!["lg".to_string()]));
}
