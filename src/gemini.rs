use anyhow::{Context, Result, anyhow, bail};
use native_tls::TlsConnector;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const DEFAULT_PORT: u16 = 1965;
const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct GeminiDocument {
    pub url: String,
    pub status: u16,
    pub meta: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeminiUrl {
    host: String,
    port: u16,
    path_and_query: String,
}

pub fn fetch(url: &str) -> Result<GeminiDocument> {
    fetch_following_redirects(url, 0)
}

pub fn resolve_url(base: &str, target: &str) -> Result<String> {
    let target = target.trim();
    if target.starts_with("gemini://") {
        return Ok(target.to_string());
    }
    if target.contains("://") {
        bail!("Unsupported link scheme: {}", target);
    }

    let base_url = parse_url(base)?;
    if target.starts_with('/') {
        return Ok(format!(
            "gemini://{}:{}{}",
            base_url.host, base_url.port, target
        ));
    }

    let base_path = base_url
        .path_and_query
        .split(['?', '#'])
        .next()
        .unwrap_or("/");
    let base_dir = if base_path.ends_with('/') {
        base_path.to_string()
    } else {
        base_path
            .rsplit_once('/')
            .map(|(dir, _)| format!("{dir}/"))
            .unwrap_or_else(|| "/".to_string())
    };
    Ok(format!(
        "gemini://{}:{}{}{}",
        base_url.host, base_url.port, base_dir, target
    ))
}

fn fetch_following_redirects(url: &str, depth: usize) -> Result<GeminiDocument> {
    if depth > 5 {
        bail!("Too many Gemini redirects");
    }

    let parsed = parse_url(url)?;
    let request_url = format!(
        "gemini://{}:{}{}",
        parsed.host, parsed.port, parsed.path_and_query
    );
    let mut document = fetch_once(&parsed, &request_url)?;

    if (30..40).contains(&document.status) {
        if document.meta.trim().is_empty() {
            bail!("Gemini redirect without target");
        }
        let redirect_url = resolve_url(&request_url, &document.meta)?;
        document = fetch_following_redirects(&redirect_url, depth + 1)?;
    }

    Ok(document)
}

fn fetch_once(parsed: &GeminiUrl, request_url: &str) -> Result<GeminiDocument> {
    let address = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .with_context(|| format!("Resolving {}", parsed.host))?
        .next()
        .ok_or_else(|| anyhow!("No address for {}", parsed.host))?;
    let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(10))
        .with_context(|| format!("Connecting to {}:{}", parsed.host, parsed.port))?;
    tcp.set_read_timeout(Some(Duration::from_secs(20)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))?;

    let connector = TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .context("Creating TLS connector")?;
    let mut stream = connector
        .connect(&parsed.host, tcp)
        .with_context(|| format!("TLS handshake with {}", parsed.host))?;

    stream
        .write_all(format!("{request_url}\r\n").as_bytes())
        .context("Sending Gemini request")?;
    stream.flush().context("Flushing Gemini request")?;

    let mut response = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut response)
        .context("Reading Gemini response")?;
    if response.len() > MAX_RESPONSE_BYTES {
        bail!("Gemini response too large");
    }
    parse_response(request_url, response)
}

fn parse_response(url: &str, response: Vec<u8>) -> Result<GeminiDocument> {
    let Some(header_end) = response.windows(2).position(|w| w == b"\r\n") else {
        bail!("Invalid Gemini response: missing CRLF header");
    };
    let header = std::str::from_utf8(&response[..header_end]).context("Invalid Gemini header")?;
    let mut parts = header.splitn(2, char::is_whitespace);
    let status = parts
        .next()
        .ok_or_else(|| anyhow!("Missing Gemini status"))?
        .parse::<u16>()
        .context("Invalid Gemini status")?;
    let meta = parts.next().unwrap_or("").trim().to_string();
    let body = response[header_end + 2..].to_vec();

    if !(20..30).contains(&status) && !(30..40).contains(&status) {
        bail!("Gemini status {}: {}", status, meta);
    }

    Ok(GeminiDocument {
        url: url.to_string(),
        status,
        meta,
        body,
    })
}

fn parse_url(url: &str) -> Result<GeminiUrl> {
    let rest = url
        .trim()
        .strip_prefix("gemini://")
        .ok_or_else(|| anyhow!("Not a Gemini URL: {}", url))?;
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        bail!("Gemini URL missing host");
    }

    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port
            .parse::<u16>()
            .with_context(|| format!("Invalid Gemini port: {port}"))?;
        (host, port)
    } else {
        (authority, DEFAULT_PORT)
    };
    if host.is_empty() {
        bail!("Gemini URL missing host");
    }

    let path_and_query = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    Ok(GeminiUrl {
        host: host.to_string(),
        port,
        path_and_query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_gemini_links() {
        assert_eq!(
            resolve_url("gemini://example.org:1965/dir/page.gmi", "next.gmi").unwrap(),
            "gemini://example.org:1965/dir/next.gmi"
        );
        assert_eq!(
            resolve_url("gemini://example.org:1965/dir/page.gmi", "/root.gmi").unwrap(),
            "gemini://example.org:1965/root.gmi"
        );
    }

    #[test]
    fn parses_success_response() {
        let doc = parse_response(
            "gemini://example.org:1965/",
            b"20 text/gemini\r\n# Hello\n".to_vec(),
        )
        .unwrap();
        assert_eq!(doc.status, 20);
        assert_eq!(doc.meta, "text/gemini");
        assert_eq!(doc.body, b"# Hello\n");
    }
}
