//! EML (RFC 2822 / MIME) viewer.
//! Produces both plain-text lines (for search) and styled ratatui Lines (for display).

use super::viewer_html::html_document;
use base64::decode_config;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;

const CLR_SECTION: Color = Color::Rgb(255, 216, 80);
const CLR_HDR_KEY: Color = Color::Rgb(100, 200, 255);
const CLR_HDR_VAL: Color = Color::White;
const CLR_ATTACH: Color = Color::Rgb(255, 160, 80);
const CLR_BODY: Color = Color::White;

pub(super) fn eml_lines(data: &[u8]) -> Vec<String> {
    parse_eml(data).plain
}

pub(super) fn eml_render_lines(data: &[u8]) -> Vec<Line<'static>> {
    parse_eml(data).rendered
}

struct EmlDoc {
    plain: Vec<String>,
    rendered: Vec<Line<'static>>,
}

fn parse_eml(data: &[u8]) -> EmlDoc {
    let text = String::from_utf8_lossy(data)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let (headers_raw, body_raw) = split_headers_body(&text);
    let headers = parse_headers(headers_raw);

    let mut plain: Vec<String> = Vec::new();
    let mut rendered: Vec<Line<'static>> = Vec::new();

    push_section(&mut plain, &mut rendered, " Message ");
    push_blank(&mut plain, &mut rendered);

    for key in ["From", "To", "Cc", "Reply-To", "Date", "Subject"] {
        if let Some(raw_val) = headers.get(&key.to_ascii_lowercase()) {
            let val = decode_rfc2047(raw_val);
            push_header(&mut plain, &mut rendered, key, &val);
        }
    }
    if let Some(ct) = headers.get("content-type") {
        push_header(
            &mut plain,
            &mut rendered,
            "Content-Type",
            &decode_rfc2047(ct),
        );
    }
    if let Some(cte) = headers.get("content-transfer-encoding") {
        push_header(&mut plain, &mut rendered, "Encoding", cte);
    }

    let attachments = collect_attachments(&headers, body_raw);
    if !attachments.is_empty() {
        push_blank(&mut plain, &mut rendered);
        push_section(&mut plain, &mut rendered, " Attachments ");
        push_blank(&mut plain, &mut rendered);
        for name in &attachments {
            let s = format!("  [+] {name}");
            plain.push(s.clone());
            rendered.push(Line::from(Span::styled(
                s,
                Style::default().fg(CLR_ATTACH).add_modifier(Modifier::BOLD),
            )));
        }
    }

    push_blank(&mut plain, &mut rendered);
    push_section(&mut plain, &mut rendered, " Body ");
    push_blank(&mut plain, &mut rendered);

    let body_lines = extract_best_body(&headers, body_raw);
    if body_lines.is_empty() || body_lines.iter().all(|l| l.trim().is_empty()) {
        let s = "(empty body)".to_string();
        plain.push(s.clone());
        rendered.push(Line::from(Span::styled(
            s,
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in &body_lines {
            plain.push(line.clone());
            rendered.push(Line::from(Span::styled(
                line.clone(),
                Style::default().fg(CLR_BODY),
            )));
        }
    }

    EmlDoc { plain, rendered }
}

fn push_section(plain: &mut Vec<String>, rendered: &mut Vec<Line<'static>>, title: &str) {
    let dashes = "\u{2500}".repeat(60_usize.saturating_sub(title.chars().count()));
    let full = format!("\u{2500}{title}{dashes}");
    plain.push(full.clone());
    rendered.push(Line::from(Span::styled(
        full,
        Style::default()
            .fg(CLR_SECTION)
            .add_modifier(Modifier::BOLD),
    )));
}

fn push_blank(plain: &mut Vec<String>, rendered: &mut Vec<Line<'static>>) {
    plain.push(String::new());
    rendered.push(Line::default());
}

fn push_header(plain: &mut Vec<String>, rendered: &mut Vec<Line<'static>>, key: &str, val: &str) {
    plain.push(format!("{key}: {val}"));
    rendered.push(Line::from(vec![
        Span::styled(
            format!("{key}: "),
            Style::default()
                .fg(CLR_HDR_KEY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(val.to_string(), Style::default().fg(CLR_HDR_VAL)),
    ]));
}

fn decode_rfc2047(input: &str) -> String {
    let mut result = String::new();
    let mut rem = input;
    let mut last_was_ew = false;

    loop {
        match rem.find("=?") {
            None => {
                if !(last_was_ew && rem.chars().all(|c| c.is_ascii_whitespace())) {
                    result.push_str(rem);
                }
                break;
            }
            Some(start) => {
                let before = &rem[..start];
                if last_was_ew && before.chars().all(|c| c.is_ascii_whitespace()) {
                    // skip inter-word whitespace (RFC 2047 sec 6.2)
                } else {
                    result.push_str(before);
                }
                rem = &rem[start + 2..];

                let q1 = match rem.find('?') {
                    Some(i) => i,
                    None => {
                        result.push_str("=?");
                        result.push_str(rem);
                        return result;
                    }
                };
                let charset = rem[..q1].to_string();
                rem = &rem[q1 + 1..];

                let q2 = match rem.find('?') {
                    Some(i) => i,
                    None => {
                        result.push_str(&format!("=?{charset}?"));
                        result.push_str(rem);
                        return result;
                    }
                };
                let encoding = rem[..q2].to_string();
                rem = &rem[q2 + 1..];

                let end = match rem.find("?=") {
                    Some(i) => i,
                    None => {
                        result.push_str(rem);
                        return result;
                    }
                };
                let encoded = &rem[..end];
                rem = &rem[end + 2..];

                let decoded_bytes: Option<Vec<u8>> = match encoding.to_ascii_uppercase().as_str() {
                    "B" => {
                        let compact: String = encoded
                            .chars()
                            .filter(|c| !c.is_ascii_whitespace())
                            .collect();
                        decode_config(compact.as_bytes(), base64::STANDARD).ok()
                    }
                    "Q" => Some(decode_qp_encoded_word(encoded.as_bytes())),
                    _ => None,
                };

                if let Some(bytes) = decoded_bytes {
                    result.push_str(&decode_bytes_charset(&bytes, &charset));
                } else {
                    result.push_str(&format!("=?{charset}?{encoding}?{encoded}?="));
                }
                last_was_ew = true;
            }
        }
    }

    result
}

fn decode_qp_encoded_word(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        match input[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < input.len() => {
                let hex = std::str::from_utf8(&input[i + 1..i + 3]).unwrap_or("  ");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn decode_bytes_charset(bytes: &[u8], charset: &str) -> String {
    match charset.to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" | "" => String::from_utf8_lossy(bytes).into_owned(),
        "iso-8859-1" | "iso8859-1" | "latin-1" | "latin1" => {
            bytes.iter().map(|&b| b as char).collect()
        }
        "windows-1252" | "cp1252" | "cp-1252" => bytes.iter().map(|&b| cp1252_to_char(b)).collect(),
        "iso-8859-15" | "iso8859-15" => bytes.iter().map(|&b| latin9_to_char(b)).collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn split_headers_body(text: &str) -> (&str, &str) {
    if let Some((h, b)) = text.split_once("\n\n") {
        (h, b)
    } else {
        (text, "")
    }
}

fn parse_headers(headers: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut cur_name = String::new();
    let mut cur_val = String::new();
    for line in headers.lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !cur_name.is_empty() {
            cur_val.push(' ');
            cur_val.push_str(line.trim());
        } else {
            if !cur_name.is_empty() {
                map.insert(cur_name.clone(), cur_val.trim().to_string());
            }
            match line.split_once(':') {
                Some((name, val)) => {
                    cur_name = name.trim().to_ascii_lowercase();
                    cur_val = val.trim().to_string();
                }
                None => {
                    cur_name.clear();
                    cur_val.clear();
                }
            }
        }
    }
    if !cur_name.is_empty() {
        map.insert(cur_name, cur_val.trim().to_string());
    }
    map
}

fn header_param(value: Option<&String>, name: &str) -> Option<String> {
    let value = value?;
    for part in value.split(';').skip(1) {
        if let Some((k, v)) = part.split_once('=') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

fn extract_best_body(headers: &HashMap<String, String>, body: &str) -> Vec<String> {
    let content_type = headers
        .get("content-type")
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_else(|| "text/plain".into());
    let charset =
        header_param(headers.get("content-type"), "charset").unwrap_or_else(|| "utf-8".into());
    let transfer_encoding = headers
        .get("content-transfer-encoding")
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default();

    if content_type.starts_with("multipart/") {
        if let Some(boundary) = header_param(headers.get("content-type"), "boundary") {
            if let Some(lines) = extract_from_multipart(body, &boundary) {
                return lines;
            }
        }
    }
    decode_body(body, &transfer_encoding, &content_type, &charset)
}

fn collect_attachments(headers: &HashMap<String, String>, body: &str) -> Vec<String> {
    let content_type = headers
        .get("content-type")
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default();
    if !content_type.starts_with("multipart/") {
        return Vec::new();
    }
    let boundary = match header_param(headers.get("content-type"), "boundary") {
        Some(b) => b,
        None => return Vec::new(),
    };
    scan_parts_for_attachments(body, &boundary)
}

fn scan_parts_for_attachments(body: &str, boundary: &str) -> Vec<String> {
    let marker = format!("--{boundary}");
    let mut names: Vec<String> = Vec::new();
    for part in body.split(&marker) {
        let trimmed = part.trim_start_matches('\n');
        if trimmed.trim().is_empty() || trimmed.trim() == "--" {
            continue;
        }
        if trimmed.trim_start_matches('-').trim().is_empty() {
            continue;
        }
        let (phdr, pbody) = split_headers_body(trimmed);
        let ph = parse_headers(phdr);
        let ct = ph
            .get("content-type")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        let disp = ph
            .get("content-disposition")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        if ct.starts_with("multipart/") {
            if let Some(nb) = header_param(ph.get("content-type"), "boundary") {
                names.extend(scan_parts_for_attachments(pbody, &nb));
                continue;
            }
        }
        let is_att = disp.starts_with("attachment")
            || (!ct.is_empty() && !ct.starts_with("text/") && !ct.starts_with("multipart/"));
        if is_att {
            let raw = header_param(ph.get("content-disposition"), "filename")
                .or_else(|| header_param(ph.get("content-type"), "name"))
                .unwrap_or_else(|| ct.split(';').next().unwrap_or("file").trim().to_string());
            let name = decode_rfc2047(&raw);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn extract_from_multipart(body: &str, boundary: &str) -> Option<Vec<String>> {
    let marker = format!("--{boundary}");
    let mut best_plain: Option<Vec<String>> = None;
    let mut best_html: Option<Vec<String>> = None;
    for part in body.split(&marker) {
        let trimmed = part.trim_start_matches('\n').trim();
        if trimmed.is_empty() || trimmed == "--" || trimmed.ends_with("--") {
            continue;
        }
        if trimmed.starts_with("--") {
            continue;
        }
        let (phdr, pbody) = split_headers_body(trimmed);
        let ph = parse_headers(phdr);
        let ct = ph
            .get("content-type")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_else(|| "text/plain".into());
        let disp = ph
            .get("content-disposition")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        if disp.starts_with("attachment") {
            continue;
        }
        if ct.starts_with("multipart/") {
            if let Some(nb) = header_param(ph.get("content-type"), "boundary") {
                if let Some(lines) = extract_from_multipart(pbody, &nb) {
                    return Some(lines);
                }
            }
            continue;
        }
        let enc = ph
            .get("content-transfer-encoding")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        let cs = header_param(ph.get("content-type"), "charset").unwrap_or_else(|| "utf-8".into());
        let lines = decode_body(pbody, &enc, &ct, &cs);
        if ct.starts_with("text/plain") && best_plain.is_none() {
            best_plain = Some(lines);
        } else if ct.starts_with("text/html") && best_html.is_none() {
            best_html = Some(lines);
        }
    }
    best_plain.or(best_html)
}

fn decode_body(body: &str, encoding: &str, content_type: &str, charset: &str) -> Vec<String> {
    let decoded: String = match encoding {
        "base64" => {
            let compact: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
            decode_config(compact.as_bytes(), base64::STANDARD)
                .ok()
                .map(|b| decode_bytes_charset(&b, charset))
                .unwrap_or_else(|| String::from_utf8_lossy(body.as_bytes()).into_owned())
        }
        "quoted-printable" => decode_bytes_charset(&decode_quoted_printable(body), charset),
        _ => {
            if charset.eq_ignore_ascii_case("utf-8")
                || charset.eq_ignore_ascii_case("utf8")
                || charset.is_empty()
            {
                body.to_string()
            } else {
                decode_bytes_charset(body.as_bytes(), charset)
            }
        }
    };
    if content_type.starts_with("text/html") {
        let html = html_document(decoded.as_bytes());
        let lines: Vec<String> = html.lines.into_iter().map(|l| l.plain).collect();
        if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        }
    } else {
        let lines: Vec<String> = decoded
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .lines()
            .map(|s| s.to_string())
            .collect();
        if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        }
    }
}

fn decode_quoted_printable(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'=' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                i += 2;
            }
            b'=' if i + 2 < bytes.len() && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' => {
                i += 3;
            }
            b'=' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn cp1252_to_char(b: u8) -> char {
    const TABLE: [char; 32] = [
        '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}',
        '\u{017D}', '\u{FFFD}', '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
    ];
    if (0x80..0xA0).contains(&b) {
        TABLE[(b - 0x80) as usize]
    } else {
        b as char
    }
}

fn latin9_to_char(b: u8) -> char {
    match b {
        0xA4 => '\u{20AC}',
        0xA6 => '\u{0160}',
        0xA8 => '\u{0161}',
        0xB4 => '\u{017D}',
        0xB8 => '\u{017E}',
        0xBC => '\u{0152}',
        0xBD => '\u{0153}',
        0xBE => '\u{0178}',
        _ => b as char,
    }
}
