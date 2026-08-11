// CurlFtpFS - an FTP filesystem using libcurl and FUSE.
// Copyright (C) 2006-2026 CurlFtpFS contributors.
// Distributed under GPL-2.0-or-later.

mod filesystem;
mod options;
mod remote;

use fuser::MountOption;
use options::{Args, ParseResult};
use std::{ffi::OsString, process::ExitCode};

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<OsString> = std::env::args_os().collect();
    let parsed = match Args::parse(&args) {
        Ok(ParseResult::Run(v)) => v,
        Ok(ParseResult::Help) => {
            print!("{}", options::HELP);
            return ExitCode::SUCCESS;
        }
        Ok(ParseResult::Version) => {
            println!(
                "curlftpfs {} libcurl/{}",
                env!("CARGO_PKG_VERSION"),
                curl::Version::get().version()
            );
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("curlftpfs: {e}\nTry 'curlftpfs --help' for more information.");
            return ExitCode::from(2);
        }
    };

    let remote = match remote::Remote::new(parsed.url.clone(), parsed.curl.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("curlftpfs: {e}");
            return ExitCode::FAILURE;
        }
    };
    let fs = filesystem::FtpFs::new(
        remote,
        parsed.cache_timeout,
        parsed.mountpoint.clone(),
        parsed.curl.transform_symlinks,
    );
    let mut mount_options = vec![
        MountOption::FSName("curlftpfs".into()),
        MountOption::Subtype("curlftpfs".into()),
    ];
    mount_options.extend(parsed.mount_options);
    if let Err(e) = fuser::mount2(fs, &parsed.mountpoint, &mount_options) {
        eprintln!(
            "curlftpfs: cannot mount {}: {e}",
            parsed.mountpoint.display()
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
