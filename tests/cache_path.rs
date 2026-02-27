use curlftpfs::cache::{parse_cache_options, Cache, CacheLookup};
use curlftpfs::parser::FileStat;
use curlftpfs::path_utils::{
    get_dir_path, get_file_name, get_full_path, get_fulldir_path, PathContext,
};

fn stat() -> FileStat {
    FileStat {
        mode: 0o100644,
        nlink: 1,
        size: 12,
        blksize: 4096,
        blocks: 8,
        mtime_raw: "Jan 01 2001".to_string(),
    }
}

#[test]
fn cache_add_get_and_invalidate() {
    let cache = Cache::default();
    cache.add_attr("/x", Some(stat()));
    assert!(matches!(cache.get_attr("/x"), CacheLookup::Hit(_)));

    cache.add_link("/x", "target", 1024);
    assert_eq!(cache.get_link("/x"), CacheLookup::Hit("target".to_string()));

    cache.add_dir("/", vec!["x".to_string(), "y".to_string()]);
    assert_eq!(
        cache.get_dir("/"),
        CacheLookup::Hit(vec!["x".to_string(), "y".to_string()])
    );

    cache.invalidate("/x");
    assert_eq!(cache.get_attr("/x"), CacheLookup::Miss);
}

#[test]
fn cache_option_parser_matches_legacy_flags() {
    let args = vec![
        "cache=no".to_string(),
        "cache_timeout=20".to_string(),
        "cache_dir_timeout=30".to_string(),
    ];
    let opts = parse_cache_options(&args);
    assert!(!opts.on);
    assert_eq!(opts.stat_timeout, 20);
    assert_eq!(opts.link_timeout, 20);
    assert_eq!(opts.dir_timeout, 30);
}

#[test]
fn path_utils_match_expected_shapes() {
    let ctx = PathContext {
        host: "ftp://example/".to_string(),
        codepage: None,
        iocharset: None,
    };

    assert_eq!(get_file_name("/a/b/c.txt", &ctx), "c.txt");
    assert_eq!(get_full_path("/a/b/c.txt", &ctx), "ftp://example/a/b/c.txt");
    assert_eq!(get_fulldir_path("/a/b", &ctx), "ftp://example/a/b/");
    assert_eq!(get_fulldir_path("/", &ctx), "ftp://example/");
    assert_eq!(get_dir_path("/a/b/c.txt", &ctx), "ftp://example/a/b/");
}
