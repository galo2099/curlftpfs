use crate::cache::Cache;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpFsConfig {
    pub blksize: u64,
    pub symlink_prefix: Option<String>,
}

impl Default for FtpFsConfig {
    fn default() -> Self {
        Self {
            blksize: 4096,
            symlink_prefix: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub mode: u32,
    pub nlink: u64,
    pub size: u64,
    pub blksize: u64,
    pub blocks: u64,
    pub mtime_raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEntry {
    pub name: String,
    pub full_path: String,
    pub stat: FileStat,
    pub symlink_target: Option<String>,
}

const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

pub fn parse_dir(list: &str, dir: &str, cfg: &FtpFsConfig) -> Vec<ParsedEntry> {
    list.lines()
        .filter_map(|line| parse_line(line.trim_end_matches('\r'), dir, cfg))
        .collect()
}

pub fn parse_dir_for_name(
    list: &str,
    dir: &str,
    name: &str,
    cfg: &FtpFsConfig,
) -> Option<ParsedEntry> {
    if name.is_empty() {
        return Some(ParsedEntry {
            name: String::new(),
            full_path: dir.to_string(),
            stat: make_stat(S_IFDIR | 0o755, 1, 1024, "root".to_string(), cfg.blksize),
            symlink_target: None,
        });
    }

    parse_dir(list, dir, cfg)
        .into_iter()
        .find(|entry| entry.name == name)
}

pub fn parse_dir_and_cache(
    list: &str,
    dir: &str,
    cfg: &FtpFsConfig,
    cache: &Cache,
) -> Vec<ParsedEntry> {
    let entries = parse_dir(list, dir, cfg);
    let dir_names = entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    cache.add_dir(dir, dir_names);

    for entry in &entries {
        cache.add_attr(&entry.full_path, Some(entry.stat.clone()));
        if let Some(target) = &entry.symlink_target {
            cache.add_link(&entry.full_path, target, target.len() + 1);
        }
    }

    entries
}

fn parse_line(line: &str, dir: &str, cfg: &FtpFsConfig) -> Option<ParsedEntry> {
    parse_unix(line, dir, cfg).or_else(|| parse_windows(line, dir, cfg))
}

fn parse_unix(line: &str, dir: &str, cfg: &FtpFsConfig) -> Option<ParsedEntry> {
    let spans = token_spans(line);
    if spans.len() < 8 {
        return None;
    }

    let mode_str = token_at(line, spans[0]);
    if mode_str.len() < 10 {
        return None;
    }

    let mon_idx = spans
        .iter()
        .position(|span| is_month(token_at(line, *span)))?;
    if mon_idx < 2 || spans.len() <= mon_idx + 2 {
        return None;
    }

    let nlink = token_at(line, spans[1]).parse().ok().unwrap_or(1);
    let size: u64 = token_at(line, spans[mon_idx - 1]).parse().ok()?;
    let month = token_at(line, spans[mon_idx]);
    let day = token_at(line, spans[mon_idx + 1]);
    let year_or_time = token_at(line, spans[mon_idx + 2]);

    let after_time = spans[mon_idx + 2].1;
    let raw_name = if after_time < line.len() {
        let mut i = after_time;
        // Mirror C "%*c%1023c": consume a single separator char after date field.
        i = (i + 1).min(line.len());
        line[i..].to_string()
    } else {
        String::new()
    };

    let (name_and_link, symlink_target) = if let Some((name, target)) = raw_name.split_once(" -> ")
    {
        let target = match (&cfg.symlink_prefix, target.starts_with('/')) {
            (Some(prefix), true) => format!("{prefix}{target}"),
            _ => target.to_string(),
        };
        (name.to_string(), Some(target))
    } else {
        (raw_name, None)
    };

    let file_type = match mode_str.as_bytes()[0] as char {
        'd' => S_IFDIR,
        'l' => S_IFLNK,
        _ => S_IFREG,
    };

    Some(ParsedEntry {
        full_path: format!("{dir}{name_and_link}"),
        name: name_and_link,
        stat: make_stat(
            file_type | mode_to_perms(mode_str),
            nlink,
            size,
            format!("{month} {day} {year_or_time}"),
            cfg.blksize,
        ),
        symlink_target,
    })
}

fn parse_windows(line: &str, dir: &str, cfg: &FtpFsConfig) -> Option<ParsedEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    let date = parts[0];
    let time = parts[1];
    let size_or_dir = parts[2];
    let name = parts[3..].join(" ");

    let (file_type, size) = if size_or_dir == "<DIR>" {
        (S_IFDIR, 0)
    } else {
        (S_IFREG, size_or_dir.parse().ok()?)
    };

    Some(ParsedEntry {
        full_path: format!("{dir}{name}"),
        name,
        stat: make_stat(file_type, 1, size, format!("{date} {time}"), cfg.blksize),
        symlink_target: None,
    })
}

fn mode_to_perms(mode: &str) -> u32 {
    let mut perms = 0u32;
    for (i, c) in mode.chars().skip(1).take(9).enumerate() {
        if c != '-' {
            perms |= 1 << (8 - i);
        }
    }
    perms
}

fn make_stat(mode: u32, nlink: u64, size: u64, mtime_raw: String, blksize: u64) -> FileStat {
    let blocks = if blksize == 0 {
        0
    } else {
        ((size + blksize - 1) & !(blksize - 1)) >> 9
    };

    FileStat {
        mode,
        nlink,
        size,
        blksize,
        blocks,
        mtime_raw,
    }
}

fn is_month(token: &str) -> bool {
    matches!(
        token,
        "Jan"
            | "Feb"
            | "Mar"
            | "Apr"
            | "May"
            | "Jun"
            | "Jul"
            | "Aug"
            | "Sep"
            | "Oct"
            | "Nov"
            | "Dec"
    )
}

fn token_spans(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        out.push((start, i));
    }
    out
}

fn token_at(line: &str, span: (usize, usize)) -> &str {
    &line[span.0..span.1]
}
