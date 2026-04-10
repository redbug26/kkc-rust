use super::{HtmlDocument, HtmlLine, HtmlLinkRef, HtmlSpan};
use std::collections::HashMap;

pub(super) fn html_document(data: &[u8]) -> HtmlDocument {
    let input = String::from_utf8_lossy(data).into_owned();
    let chars: Vec<char> = input.chars().collect();
    let mut lines: Vec<HtmlLine> = vec![HtmlLine { spans: Vec::new(), plain: String::new() }];
    let mut anchors = HashMap::new();
    let mut links = Vec::new();
    let mut in_pre = false;
    let mut current_href: Option<String> = None;
    let mut i = 0usize;
    let mut collapse_space = true;

    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let raw_tag: String = chars[i + 1..j].iter().collect();
            let tag = raw_tag.trim();
            let lower = tag.to_ascii_lowercase();

            if lower.starts_with("a ") {
                if let Some(name) = attr_value(tag, "name").or_else(|| attr_value(tag, "id")) {
                    anchors.insert(name.to_ascii_lowercase(), lines.len().saturating_sub(1));
                }
                current_href = attr_value(tag, "href");
            } else if lower.starts_with("/a") {
                current_href = None;
            } else if lower.starts_with("br")
                || lower.starts_with("/p")
                || lower.starts_with("p")
                || lower.starts_with("/div")
                || lower.starts_with("div")
                || lower.starts_with("/h")
                || lower.starts_with("h1")
                || lower.starts_with("h2")
                || lower.starts_with("h3")
                || lower.starts_with("li")
                || lower.starts_with("hr")
            {
                push_html_line(&mut lines);
                collapse_space = true;
            } else if lower.starts_with("pre") {
                in_pre = true;
                push_html_line(&mut lines);
            } else if lower.starts_with("/pre") {
                in_pre = false;
                push_html_line(&mut lines);
            }

            i = j + 1;
            continue;
        }

        if chars[i] == '&' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != ';' && j - i < 10 {
                j += 1;
            }
            if j < chars.len() && chars[j] == ';' {
                let entity: String = chars[i + 1..j].iter().collect();
                append_html_text(&mut lines, &decode_entity(&entity), current_href.clone(), &mut links, &mut collapse_space, in_pre);
                i = j + 1;
                continue;
            }
        }

        let ch = chars[i];
        if ch == '\n' {
            push_html_line(&mut lines);
            collapse_space = true;
        } else {
            append_html_text(&mut lines, &ch.to_string(), current_href.clone(), &mut links, &mut collapse_space, in_pre);
        }
        i += 1;
    }

    while lines.last().is_some_and(|line| line.spans.is_empty() && line.plain.is_empty()) && lines.len() > 1 {
        lines.pop();
    }

    HtmlDocument { lines, anchors, links }
}

fn append_html_text(
    lines: &mut [HtmlLine],
    text: &str,
    href: Option<String>,
    links: &mut Vec<HtmlLinkRef>,
    collapse_space: &mut bool,
    in_pre: bool,
) {
    let line_idx = lines.len() - 1;
    let line = lines.last_mut().expect("html line exists");
    let normalized = if in_pre {
        text.to_string()
    } else if text.chars().all(char::is_whitespace) {
        if *collapse_space {
            String::new()
        } else {
            *collapse_space = true;
            " ".to_string()
        }
    } else {
        *collapse_space = false;
        text.to_string()
    };

    if normalized.is_empty() {
        return;
    }

    if let Some(last) = line.spans.last_mut()
        && last.href == href
    {
        last.text.push_str(&normalized);
    } else {
        if let Some(target) = href.clone() {
            links.push(HtmlLinkRef { line: line_idx, href: target });
        }
        line.spans.push(HtmlSpan { text: normalized.clone(), href });
    }
    line.plain.push_str(&normalized);
}

fn push_html_line(lines: &mut Vec<HtmlLine>) {
    if lines.last().is_some_and(|line| line.spans.is_empty() && line.plain.is_empty()) {
        return;
    }
    lines.push(HtmlLine { spans: Vec::new(), plain: String::new() });
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{}=", attr);
    let start = lower.find(&needle)?;
    let value = &tag[start + needle.len()..];
    let value = value.trim_start();
    if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = value.strip_prefix('\'') {
        let end = rest.find('\'')?;
        Some(rest[..end].to_string())
    } else {
        let end = value.find(char::is_whitespace).unwrap_or(value.len());
        Some(value[..end].to_string())
    }
}

fn decode_entity(entity: &str) -> String {
    match entity.to_ascii_lowercase().as_str() {
        "nbsp" => " ".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "amp" => "&".into(),
        "quot" => "\"".into(),
        _ => format!("&{};", entity),
    }
}
