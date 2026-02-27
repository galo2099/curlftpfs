use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser, Subcommand};

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

#[derive(Debug, Parser)]
#[command(
    name = "curlftpfs",
    about = "Rust curlftpfs port",
    after_help = "The mount mode uses the Rust FUSE implementation."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Parse an FTP LIST output file
    Parse { listing_file: PathBuf },
    /// Mount an FTP site using the Rust FUSE implementation
    Mount {
        ftp_site: String,
        mountpoint: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra_options: Vec<String>,
    },
}

pub fn parse_args(args: &[String]) -> Result<CliMode, String> {
    let cli = Cli::try_parse_from(args).map_err(|e| e.to_string())?;
    Ok(match cli.command {
        Commands::Parse { listing_file } => CliMode::Parse { listing_file },
        Commands::Mount {
            ftp_site,
            mountpoint,
            extra_options,
        } => CliMode::Mount {
            ftp_site,
            mountpoint,
            extra_options,
        },
    })
}

pub fn usage() -> String {
    Cli::command().render_help().to_string()
}

pub fn is_self_delegate(current_exe: &Path, legacy_bin: &Path) -> bool {
    match (current_exe.canonicalize(), legacy_bin.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
