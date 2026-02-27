use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliMode {
    Parse {
        listing_file: PathBuf,
    },
    Mount {
        ftp_site: String,
        mountpoint: String,
        extra_options: Vec<String>,
    },
}

pub fn parse_args(args: &[String]) -> Result<CliMode, String> {
    if args.len() < 2 {
        return Err(usage());
    }

    match args[1].as_str() {
        "parse" => {
            let Some(path) = args.get(2) else {
                return Err(format!("missing listing file\n\n{}", usage()));
            };
            Ok(CliMode::Parse {
                listing_file: PathBuf::from(path),
            })
        }
        "mount" => {
            let Some(site) = args.get(2) else {
                return Err(format!("missing ftp site\n\n{}", usage()));
            };
            let Some(mountpoint) = args.get(3) else {
                return Err(format!("missing mountpoint\n\n{}", usage()));
            };
            Ok(CliMode::Mount {
                ftp_site: site.clone(),
                mountpoint: mountpoint.clone(),
                extra_options: args[4..].to_vec(),
            })
        }
        _ => Err(usage()),
    }
}

pub fn usage() -> String {
    "usage:\n  curlftpfs parse <listing-file>\n  curlftpfs mount <ftp-site> <mountpoint> [legacy curlftpfs options...]\n\nThe mount mode uses the Rust FUSE implementation.".to_string()
}

pub fn is_self_delegate(current_exe: &Path, legacy_bin: &Path) -> bool {
    match (current_exe.canonicalize(), legacy_bin.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
