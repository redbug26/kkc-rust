use super::viewer_html::html_document;
use base64::decode as base64_decode;
use std::collections::HashMap;

pub(super) fn eml_lines(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data).replace("\r\n", "\n").replace('\r', "\n");
    let (headers, body) = split_headers_body(&text);
    let headers = parse_headers(headers);

    let mut out = Vec::new();
    out.push("Message".into());
    out.push(String::new());
    for key in ["From", "To", "Cc", "Date", "Subject", "Reply-To"] {
        if let Some(value) = headers.get(&key.to_ascii_lowercase()) {
            out.push(format!("{key}: {value}"));
        }
    }
    if let Some(value) = headers.get("content-type") {
        out.push(format!("Content-Type: {value}"));
    }
    if let Some(value) = headers.get("content-transfer-encoding") {
        out.push(format!("Encoding: {value}"));
    }
    out.push(String::new());
    out.push("Body".into());
    out.push(String::new());

    let body_lines = extract_best_body(&headers, body);
    if body_lines.is_empty() {
        out.push("(empty body)".into());
    } else {
        out.extend(body_lines);
    }
    out
}

fn split_headers_body(text: &str) -> (&str, &str) {
    if let Some((headers, body)) = text.split_once("\n\n") {
        (headers, body)
    } else {
        (text, "")
    }
}

fn parse_headers(headers: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut current_name = String::new();
    let mut current_value = String::new();

    for line in headers.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if !current_value.is_empty() {
                current_value.push(' ');
                current_value.push_str(line.trim());
            }
            continue;
        }
        if !current_name.is_empty() {
            map.insert(current_name.clone(), current_value.trim().to_string());
        }
        if let Some((name, value)) = line.split_once(':') {
            current_name = name.trim().to_ascii_lowercase();
            current_value = value.trim().to_string();
        } else {
            current_name.clear();
            current_value.clear();
        }
    }
    if !current_name.is_empty() {
        map.insert(current_name, current_value.trim().to_string());
    }
    map
}

fn extract_best_body(headers: &HashMap<String, String>, body: &str) -> Vec<String> {
    let content_type = headers
        .get("content-type")
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_else(|| "text/plain".into());
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

    decode_body(body, &transfer_encoding, &content_type)
}

fn extract_from_multipart(body: &str, boundary: &str) -> Option<Vec<String>> {
    let marker = format!("--{boundary}");
    let end_marker = format!("--{boundary}--");
    let mut best_plain: Option<Vec<String>> = None;
    let mut best_html: Option<Vec<String>> = None;

    for part in body.split(&marker) {
        let trimmed = part.trim_start_matches('\n').trim();
        if trimmed.is_empty() || trimmed == "--" || trimmed == end_marker {
            continue;
        }
        if trimmed.starts_with("--") {
            continue;
        }
        let (part_headers_raw, part_body) = split_headers_body(trimmed);
        let part_headers = parse_headers(part_headers_raw);
        let content_type = part_headers
            .get("content-type")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_else(|| "text/plain".into());
        if content_type.starts_with("multipart/") {
            if let Some(nested_boundary) = header_param(part_headers.get("content-type"), "boundary")
                && let Some(lines) = extract_from_multipart(part_body, &nested_boundary)
            {
                return Some(lines);
            }
            continue;
        }
        let encoding = part_headers
            .get("content-transfer-encoding")
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        let lines = decode_body(part_body, &encoding, &content_type);
        if content_type.starts_with("text/plain") && best_plain.is_none() {
            best_plain = Some(lines);
        } else if content_type.starts_with("text/html") && best_html.is_none() {
            best_html = Some(lines);
        }
    }

    best_plain.or(best_html)
}

fn decode_body(body: &str, encoding: &str, content_type: &str) -> Vec<String> {
    let decoded = match encoding {
        "base64" => {
            let compact = body.lines().collect::<String>();
            base64_decode(compact.as_bytes())
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|| body.to_string())
        }
        "quoted-printable" => decode_quoted_printable(body),
        _ => body.to_string(),
    };

    if content_type.starts_with("text/html") {
        let html = html_document(decoded.as_bytes());
        let lines = html
            .lines
            .into_iter()
            .map(|line| line.plain)
            .collect::<Vec<_>>();
        if lines.is_empty() { vec![String::new()] } else { lines }
    } else {
        let lines = decoded
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        if lines.is_empty() { vec![String::new()] } else { lines }
    }
}

fn header_param(value: Option<&String>, name: &str) -> Option<String> {
    let value = value?;
    for part in value.split(';').skip(1) {
        let (k, v) = part.split_once('=')?;
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn decode_quoted_printable(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'=' if i + 2 < bytes.len() => {
                if bytes[i + 1] == b'\n' {
                    i += 2;
                    continue;
                }
                if bytes[i + 1] == b'\r' && i + 2 < bytes.len() && bytes[i + 2] == b'\n' {
                    i += 3;
                    continue;
                }
                let hex = &input[i + 1..i + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
