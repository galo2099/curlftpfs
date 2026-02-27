use std::fs;
use std::path::PathBuf;

use curlftpfs::cli::{parse_args, CliMode};
use curlftpfs::parser::{parse_dir, FtpFsConfig};
use curlftpfs::rustfs::FtpFuseFs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = match parse_args(&args) {
        Ok(mode) => mode,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    match mode {
        CliMode::Parse { listing_file } => run_parse(&listing_file),
        CliMode::Mount {
            ftp_site,
            mountpoint,
            extra_options: _,
        } => run_mount(&ftp_site, &mountpoint),
    }
}

fn run_parse(path: &PathBuf) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to read {}: {e}", path.display());
            std::process::exit(1);
        }
    };

    let entries = parse_dir(&content, "/", &FtpFsConfig::default());
    for entry in entries {
        println!(
            "{}\tmode={:o}\tsize={}\tmtime={}{}",
            entry.name,
            entry.stat.mode,
            entry.stat.size,
            entry.stat.mtime_raw,
            entry
                .symlink_target
                .as_ref()
                .map(|t| format!("\t-> {t}"))
                .unwrap_or_default()
        );
    }
}

fn run_mount(ftp_site: &str, mountpoint: &str) {
    let fs = match FtpFuseFs::new(ftp_site) {
        Ok(fs) => fs,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = fs.mount(mountpoint) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
