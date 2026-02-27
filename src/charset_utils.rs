use iconv::{Iconv, IconvError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharsetError {
    Open {
        from: String,
        to: String,
        message: String,
    },
    Convert {
        from: String,
        to: String,
        message: String,
    },
    InvalidUtf8,
}

pub fn convert_charsets(from: &str, to: &str, input: &str) -> Result<String, CharsetError> {
    if from.eq_ignore_ascii_case(to) {
        return Ok(input.to_string());
    }

    let mut cd = Iconv::new(from, to).map_err(|e| CharsetError::Open {
        from: from.to_string(),
        to: to.to_string(),
        message: format_iconv_error(e),
    })?;

    let inbuf = input.as_bytes();
    let mut outbuf = vec![0_u8; inbuf.len().saturating_mul(4).saturating_add(32)];

    let (_, written, _) = cd
        .convert(inbuf, outbuf.as_mut_slice())
        .map_err(|(_, _, e)| CharsetError::Convert {
            from: from.to_string(),
            to: to.to_string(),
            message: format_iconv_error(e),
        })?;

    outbuf.truncate(written);
    String::from_utf8(outbuf).map_err(|_| CharsetError::InvalidUtf8)
}

fn format_iconv_error(e: IconvError) -> String {
    match e {
        IconvError::ConversionNotSupport => "conversion not supported".to_string(),
        IconvError::NotSufficientOutput => "insufficient output buffer".to_string(),
        IconvError::IncompleteInput => "incomplete input".to_string(),
        IconvError::InvalidInput => "invalid input".to_string(),
        IconvError::OsError(code) => format!("os error {code}"),
    }
}
