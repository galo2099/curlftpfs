# Rust Port Status

## Implemented in Rust

- Filesystem implementation now targets the `fuser` API surface
- FUSE-like lifecycle and operation adapter layer
- FTP command sequencing for getattr/readdir/readlink/read via Rust service orchestration
- Write-path semantics with open/write/flush/release staging
- libcurl-compatible transport layer via curl CLI adapter + trait-based transport abstraction
- Cache integration across parser and operation service flows
- FTP LIST line parsing (UNIX and Windows listing styles)
- Symlink target extraction from UNIX listings
- Permission/type bit derivation and basic stat-like metadata shaping
- In-memory cache model with TTL handling for attrs/dirs/links and option parsing
- Path utility helpers (`get_file_name`, `get_full_path`, `get_fulldir_path`, `get_dir_path`)
- FTPFS mount-option parsing model for legacy curlftpfs flags

## Not yet implemented in Rust

- None (tracked migration tasks currently completed in Rust modules)

## Source of truth for current full functionality

The complete functional implementation remains in the C codebase for now:

- `ftpfs.c`
- `cache.c`
- `path_utils.c`
- `charset_utils.c`
- `ftpfs-ls.c`

This document exists to make migration scope explicit and avoid implying feature parity too early.
