pub(super) fn parse_hex_query(query: &str) -> Option<Vec<u8>> {
    let compact = query
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect::<String>();
    if compact.is_empty()
        || compact.len() % 2 != 0
        || !compact.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    let bytes = compact.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let pair = std::str::from_utf8(&bytes[i..i + 2]).ok()?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
        i += 2;
    }
    Some(out)
}
