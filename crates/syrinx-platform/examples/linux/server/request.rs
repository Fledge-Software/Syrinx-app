//! Reads bounded synthetic HTTP headers without retaining or printing their raw values.

use std::io::{self, BufRead, BufReader, Read};

pub(super) struct Request {
    pub route: String,
    pub cookie: bool,
}

pub(super) fn read(stream: impl Read) -> io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream.take(16 * 1024));
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut words = line.split_whitespace();
    if words.next() != Some("GET") {
        return Err(invalid());
    }
    let route = words
        .next()
        .filter(|route| route.starts_with('/'))
        .ok_or_else(invalid)?
        .to_owned();
    if !matches!(words.next(), Some("HTTP/1.0" | "HTTP/1.1")) || words.next().is_some() {
        return Err(invalid());
    }
    let mut cookie = false;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || !line.ends_with("\r\n") {
            return Err(invalid());
        }
        if line == "\r\n" {
            return Ok(Some(Request { route, cookie }));
        }
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("Cookie") {
            cookie |= value
                .trim()
                .split(';')
                .any(|pair| pair.trim() == "syrinx_process=synthetic");
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Malformed or oversized synthetic HTTP headers",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_match_is_an_exact_pair_not_a_body_or_header_substring() {
        for header in [
            "Cookie: syrinx_process=synthetic-other",
            "Cookie: other_syrinx_process=synthetic",
            "X-Cookie: syrinx_process=synthetic",
            "Cookie: unrelated=synthetic",
        ] {
            let raw = format!(
                "GET /cookie/process/read HTTP/1.1\r\n{header}\r\n\r\nsyrinx_process=synthetic"
            );
            assert!(!read(raw.as_bytes()).unwrap().unwrap().cookie);
        }
        let request = read(b"GET /cookie/process/read HTTP/1.1\r\ncookie: x=y\r\nCOOKIE: syrinx_process=synthetic; z=q\r\n\r\n".as_slice()).unwrap().unwrap();
        assert!(request.cookie);
        assert_eq!(request.route, "/cookie/process/read");
    }

    #[test]
    fn truncated_and_oversized_headers_fail_closed() {
        for raw in [
            "GET / HTTP/1.1\r\nCookie: syrinx_process=synthetic\r\n".to_owned(),
            format!(
                "GET / HTTP/1.1\r\nX-Fill: {}\r\n\r\n",
                "x".repeat(16 * 1024)
            ),
        ] {
            assert_eq!(
                read(raw.as_bytes()).err().unwrap().kind(),
                io::ErrorKind::InvalidData
            );
        }
        assert!(read(b"".as_slice()).unwrap().is_none());
    }
}
