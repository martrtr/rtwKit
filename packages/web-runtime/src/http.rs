//! Bounded HTTP upgrade and RFC 6455 framing over the existing nonblocking TCP imports.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha1::{Digest, Sha1};

use crate::wire::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};

pub const MAX_HTTP_BYTES: usize = 16 * 1024;
pub struct Head {
    pub path: String,
    pub websocket: Option<String>,
}

pub fn parse_head(bytes: &[u8], port: u16) -> Result<Option<(Head, usize)>, &'static str> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    let length = match request.parse(bytes).map_err(|_| "invalid HTTP")? {
        httparse::Status::Partial => {
            if bytes.len() > MAX_HTTP_BYTES {
                return Err("headers too large");
            }
            return Ok(None);
        }
        httparse::Status::Complete(length) => length,
    };
    if length > MAX_HTTP_BYTES {
        return Err("headers too large");
    }
    let path = request.path.ok_or("missing path")?;
    if request.method != Some("GET")
        || request.version != Some(1)
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['#', '?'])
    {
        return Err("invalid request target");
    }
    let field = |name: &str| -> Result<Option<&str>, &'static str> {
        let mut matches = request
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name));
        let first = matches.next();
        if matches.next().is_some() {
            return Err("duplicate header");
        }
        first
            .map(|h| {
                std::str::from_utf8(h.value)
                    .map(str::trim)
                    .map_err(|_| "invalid header")
            })
            .transpose()
    };
    let expected = format!("127.0.0.1:{port}");
    if field("host")? != Some(expected.as_str())
        || field("transfer-encoding")?.is_some()
        || field("content-length")?.is_some_and(|v| v != "0")
    {
        return Err("invalid host or body");
    }
    let websocket = if path == "/bridge" {
        let origin = format!("http://{expected}");
        if field("origin")? != Some(origin.as_str())
            || !field("upgrade")?.is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
            || !field("connection")?.is_some_and(|v| {
                v.split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
            })
            || field("sec-websocket-version")? != Some("13")
        {
            return Err("invalid websocket upgrade");
        }
        let key = field("sec-websocket-key")?.ok_or("missing websocket key")?;
        if STANDARD
            .decode(key)
            .map_err(|_| "invalid websocket key")?
            .len()
            != 16
        {
            return Err("invalid websocket key");
        }
        let mut hash = Sha1::new();
        hash.update(key.as_bytes());
        hash.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        Some(STANDARD.encode(hash.finalize()))
    } else {
        if field("upgrade")?.is_some() {
            return Err("unexpected upgrade");
        }
        None
    };
    Ok(Some((
        Head {
            path: path.to_owned(),
            websocket,
        },
        length,
    )))
}

pub fn upgrade(accept: &str) -> Vec<u8> {
    format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").into_bytes()
}

/// Input must remain in the validated artifact namespace, including encoded segments.
pub fn artifact_path(value: &str) -> Result<String, &'static str> {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let byte = if raw[i] == b'%' {
            let pair = raw.get(i + 1..i + 3).ok_or("invalid URL escape")?;
            let digit = |c: u8| {
                if c.is_ascii_digit() {
                    Some(c - b'0')
                } else if (b'a'..=b'f').contains(&c.to_ascii_lowercase()) {
                    Some(c.to_ascii_lowercase() - b'a' + 10)
                } else {
                    None
                }
            };
            i += 2;
            digit(pair[0])
                .and_then(|high| digit(pair[1]).map(|low| high * 16 + low))
                .ok_or("invalid URL escape")?
        } else {
            raw[i]
        };
        if byte == 0 || byte == b'\\' || byte.is_ascii_control() {
            return Err("invalid path byte");
        }
        bytes.push(byte);
        i += 1;
    }
    let path = String::from_utf8(bytes).map_err(|_| "invalid path UTF-8")?;
    if path.starts_with('/')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("path traversal");
    }
    Ok(path)
}

pub fn mime(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

pub fn static_response(port: u16, base: &str, path: &str, bytes: &[u8], shell: bool) -> Vec<u8> {
    let origin = format!("http://127.0.0.1:{port}");
    let bundle = format!("{origin}{base}");
    let csp = if shell {
        format!(
            "default-src 'none'; base-uri 'none'; object-src 'none'; form-action 'none'; script-src {bundle}; style-src {bundle} 'unsafe-inline'; img-src {bundle} blob: data:; font-src {bundle}; connect-src ws://127.0.0.1:{port}; frame-src {origin}/bundle/; frame-ancestors 'none'; worker-src 'none'"
        )
    } else {
        format!(
            "default-src 'none'; sandbox allow-scripts; base-uri 'none'; object-src 'none'; form-action 'none'; connect-src 'none'; script-src {bundle}; style-src {bundle} 'unsafe-inline'; img-src {bundle} blob: data:; font-src {bundle}; media-src {bundle} blob:; worker-src 'none'; frame-src 'none'; frame-ancestors {origin}"
        )
    };
    let mut result=format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Security-Policy: {csp}\r\nPermissions-Policy: camera=(), microphone=(), geolocation=(), fullscreen=(), payment=(), usb=(), serial=(), display-capture=()\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\n{}Connection: close\r\n\r\n",mime(path),bytes.len(),if shell {""}else{"Access-Control-Allow-Origin: *\r\n"}).into_bytes();
    result.extend_from_slice(bytes);
    result
}
pub fn rejected() -> Vec<u8> {
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
}

pub enum Message {
    Text(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}
#[derive(Default)]
pub struct Decoder {
    fragments: Option<Vec<u8>>,
}
impl Decoder {
    pub fn next(&mut self, input: &mut Vec<u8>) -> Result<Option<Message>, &'static str> {
        loop {
            if input.len() < 2 {
                return Ok(None);
            }
            let first = input[0];
            let second = input[1];
            let opcode = first & 15;
            let fin = first & 128 != 0;
            if first & 0x70 != 0 || second & 128 == 0 {
                return Err("invalid frame flags");
            }
            let (length, offset) = match second & 127 {
                n @ 0..=125 => (n as u64, 2),
                126 => {
                    if input.len() < 4 {
                        return Ok(None);
                    }
                    let n = u16::from_be_bytes(input[2..4].try_into().unwrap()) as u64;
                    if n < 126 {
                        return Err("noncanonical frame length");
                    }
                    (n, 4)
                }
                _ => {
                    if input.len() < 10 {
                        return Ok(None);
                    }
                    let n = u64::from_be_bytes(input[2..10].try_into().unwrap());
                    if n <= u16::MAX as u64 || n >> 63 != 0 {
                        return Err("noncanonical frame length");
                    }
                    (n, 10)
                }
            };
            if length > MAX_REQUEST_BYTES as u64 || (opcode >= 8 && (!fin || length > 125)) {
                return Err("frame too large");
            }
            let length = length as usize;
            let end = offset + 4 + length;
            if input.len() < end {
                return Ok(None);
            }
            let mask: [u8; 4] = input[offset..offset + 4].try_into().unwrap();
            let payload: Vec<u8> = input[offset + 4..end]
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[i % 4])
                .collect();
            input.drain(..end);
            match opcode {
                8 => {
                    if length == 1 {
                        return Err("invalid close frame");
                    }
                    return Ok(Some(Message::Close));
                }
                9 => return Ok(Some(Message::Ping(payload))),
                10 => return Ok(Some(Message::Pong)),
                1 => {
                    if self.fragments.is_some() {
                        return Err("overlapping message");
                    }
                    if fin {
                        std::str::from_utf8(&payload).map_err(|_| "invalid UTF-8")?;
                        return Ok(Some(Message::Text(payload)));
                    }
                    self.fragments = Some(payload);
                }
                0 => {
                    let aggregate = self.fragments.as_mut().ok_or("unexpected continuation")?;
                    if aggregate.len() + payload.len() > MAX_REQUEST_BYTES {
                        return Err("message too large");
                    }
                    aggregate.extend(payload);
                    if fin {
                        let result = self.fragments.take().unwrap();
                        std::str::from_utf8(&result).map_err(|_| "invalid UTF-8")?;
                        return Ok(Some(Message::Text(result)));
                    }
                }
                _ => return Err("unsupported frame opcode"),
            }
        }
    }
}
pub fn frame(opcode: u8, payload: &[u8]) -> Result<Vec<u8>, &'static str> {
    if payload.len() > MAX_RESPONSE_BYTES {
        return Err("response too large");
    }
    let mut bytes = vec![0x80 | opcode];
    if payload.len() <= 125 {
        bytes.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        bytes.push(126);
        bytes.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        bytes.push(127);
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn masked(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![
            opcode | if fin { 128 } else { 0 },
            128 | payload.len() as u8,
        ];
        frame.extend_from_slice(&[1, 2, 3, 4]);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ ([1, 2, 3, 4][i % 4])),
        );
        frame
    }
    #[test]
    fn upgrade_checks_exact_host_origin_and_rfc_accept() {
        // RFC 6455 public example, encoded from its documented plaintext nonce.
        let nonce = STANDARD.encode(b"the sample nonce");
        let request = format!(
            "GET /bridge HTTP/1.1\r\nHost: 127.0.0.1:42\r\nOrigin: http://127.0.0.1:42\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {nonce}\r\n\r\n"
        );
        let (head, _) = parse_head(request.as_bytes(), 42).unwrap().unwrap();
        assert_eq!(
            head.websocket.as_deref(),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
        for (from, to) in [
            ("127.0.0.1:42", "localhost:42"),
            ("http://127.0.0.1:42", "null"),
            ("GET /bridge", "GET http://127.0.0.1:42/bridge"),
        ] {
            let bad = request.replacen(from, to, 1);
            assert!(parse_head(bad.as_bytes(), 42).is_err());
        }
        for path in [
            "../secret",
            "a/%2e%2e/secret",
            "%2fsecret",
            "a%5cb",
            "a%00b",
        ] {
            assert!(artifact_path(path).is_err());
        }
    }
    #[test]
    fn fragments_and_control_frames_remain_bounded_and_masked() {
        let mut decoder = Decoder::default();
        let mut bytes = masked(1, false, b"hel");
        bytes.extend(masked(9, true, b"ping"));
        bytes.extend(masked(0, true, b"lo"));
        assert!(matches!(
            decoder.next(&mut bytes).unwrap(),
            Some(Message::Ping(_))
        ));
        assert!(
            matches!(decoder.next(&mut bytes).unwrap(),Some(Message::Text(payload)) if payload==b"hello")
        );
        let mut unmasked = vec![0x81, 0];
        assert!(decoder.next(&mut unmasked).is_err());
        let mut huge = vec![0x81, 0xff];
        huge.extend_from_slice(&(MAX_REQUEST_BYTES as u64 + 1).to_be_bytes());
        assert!(decoder.next(&mut huge).is_err());
    }
    #[test]
    fn panel_documents_stay_sandboxed_when_opened_directly() {
        let response = String::from_utf8(static_response(
            42,
            "/bundle/id/",
            "entry.html",
            b"panel",
            false,
        ))
        .unwrap();
        assert!(response.contains("sandbox allow-scripts"));
        assert!(response.contains("connect-src 'none'"));
        assert!(response.contains("Access-Control-Allow-Origin: *"));
        assert!(!response.contains("allow-same-origin"));
        assert!(response.contains("frame-ancestors http://127.0.0.1:42"));
    }
}
