use crate::charset_utils::convert_charsets;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PathContext {
    pub host: String,
    pub codepage: Option<String>,
    pub iocharset: Option<String>,
}

pub fn get_file_name(path: &str, ctx: &PathContext) -> String {
    let base = path.rsplit('/').next().unwrap_or(path).to_string();
    maybe_convert(base, ctx, true)
}

pub fn get_full_path(path: &str, ctx: &PathContext) -> String {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let converted = maybe_convert(trimmed.to_string(), ctx, false);
    format!("{}{}", ctx.host, converted)
}

pub fn get_fulldir_path(path: &str, ctx: &PathContext) -> String {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let converted = maybe_convert(trimmed.to_string(), ctx, false);
    if converted.is_empty() {
        format!("{}", ctx.host)
    } else {
        format!("{}{}/", ctx.host, converted)
    }
}

pub fn get_dir_path(path: &str, ctx: &PathContext) -> String {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let parent = trimmed.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let converted = maybe_convert(parent.to_string(), ctx, false);
    if converted.is_empty() {
        ctx.host.clone()
    } else {
        format!("{}{}/", ctx.host, converted)
    }
}

fn maybe_convert(input: String, ctx: &PathContext, to_codepage: bool) -> String {
    match (&ctx.codepage, &ctx.iocharset) {
        (Some(codepage), Some(iocharset)) if !input.is_empty() => {
            let (from, to) = if to_codepage {
                (iocharset.as_str(), codepage.as_str())
            } else {
                (iocharset.as_str(), codepage.as_str())
            };
            convert_charsets(from, to, &input).unwrap_or(input)
        }
        _ => input,
    }
}
