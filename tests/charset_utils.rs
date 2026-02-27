use curlftpfs::charset_utils::convert_charsets;

#[test]
fn identity_conversion_roundtrips() {
    let out = convert_charsets("UTF-8", "UTF-8", "hello").expect("convert");
    assert_eq!(out, "hello");
}
