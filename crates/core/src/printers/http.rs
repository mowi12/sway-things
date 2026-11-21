// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The little HTTP/1.1 that IPP needs: one `POST` per connection to the local CUPS, over its
//! domain socket or the loopback address, and its answer.
//!
//! Each request has its own connection with `Connection: close`, so nothing is kept open between
//! the page's reads. Answers are read up to [`MAX_RESPONSE`] bytes, with a `Content-Length` or
//! chunked; anything else is an error.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use async_io::{Async, Timer};
use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, future};

/// The most a body may be. The answers of a CUPS with 20 printers and their supplies are about
/// 20 KiB (a printer's attributes as the snapshot asks for them are under 1 KiB, a job's under
/// 300 bytes), so this holds thousands of printers or jobs.
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
/// How many interim answers (`100 Continue`) may come before the real one.
const MAX_INTERIM: usize = 8;

/// Where CUPS listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// Its domain socket.
    Socket(PathBuf),
    /// A port on the loopback address (`localhost:631`): never another host.
    Loopback(Loopback, u16),
}

/// Which loopback address CUPS listens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loopback {
    /// `localhost`: IPv4 first, then IPv6.
    Localhost,
    /// `127.0.0.1`.
    V4,
    /// `::1`.
    V6,
}

impl Endpoint {
    /// The address of CUPS's web interface on this endpoint: CUPS serves it on its port; over
    /// the socket alone, at its default port.
    pub fn web_interface(&self) -> String {
        match self {
            Self::Socket(_) => "http://localhost:631/".to_owned(),
            Self::Loopback(..) => format!("http://{self}/"),
        }
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Socket(path) => write!(f, "{}", path.display()),
            Self::Loopback(Loopback::Localhost, port) => write!(f, "localhost:{port}"),
            Self::Loopback(Loopback::V4, port) => write!(f, "127.0.0.1:{port}"),
            Self::Loopback(Loopback::V6, port) => write!(f, "[::1]:{port}"),
        }
    }
}

/// An HTTP answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The status code (200, 401, …).
    pub status: u16,
    /// The header fields, names in lowercase.
    pub headers: Vec<(String, String)>,
    /// The body, without the chunk framing.
    pub body: Vec<u8>,
}

impl Response {
    /// The value of the header field `name` (lowercase), if there is one.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Sends `body` as an IPP request to `path` (`/`, `/admin/`, `/printers/…`) at `endpoint`, with
/// the `Authorization` header `authorization` if given, and reads the answer; gives up after
/// `timeout`.
pub async fn post(
    endpoint: &Endpoint,
    path: &str,
    body: &[u8],
    authorization: Option<&str>,
    timeout: Duration,
) -> io::Result<Response> {
    future::or(exchange(endpoint, path, body, authorization), async {
        Timer::after(timeout).await;
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "CUPS didn't answer in time",
        ))
    })
    .await
}

async fn exchange(
    endpoint: &Endpoint,
    path: &str,
    body: &[u8],
    authorization: Option<&str>,
) -> io::Result<Response> {
    let request = request(path, body, authorization);
    match endpoint {
        Endpoint::Socket(socket) => {
            let stream = Async::<UnixStream>::connect(socket).await?;
            send(stream, &request).await
        }
        Endpoint::Loopback(host, port) => {
            // Only ever the loopback address: CUPS on this machine.
            let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, *port));
            let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, *port));
            let stream = match host {
                Loopback::V4 => Async::<TcpStream>::connect(v4).await?,
                Loopback::V6 => Async::<TcpStream>::connect(v6).await?,
                Loopback::Localhost => match Async::<TcpStream>::connect(v4).await {
                    Ok(stream) => stream,
                    Err(_) => Async::<TcpStream>::connect(v6).await?,
                },
            };
            send(stream, &request).await
        }
    }
}

/// The bytes of the request.
fn request(path: &str, body: &[u8], authorization: Option<&str>) -> Vec<u8> {
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/ipp\r\n\
         Content-Length: {}\r\nConnection: close\r\nUser-Agent: Sway-Things\r\n",
        body.len()
    );
    if let Some(authorization) = authorization {
        head.push_str("Authorization: ");
        head.push_str(authorization);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Writes `request` to `stream` and reads the answer.
async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    request: &[u8],
) -> io::Result<Response> {
    stream.write_all(request).await?;
    stream.flush().await?;
    let mut reader = Reader::default();
    let mut chunk = vec![0u8; 16384];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return reader.finish()?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "CUPS closed the connection")
            });
        }
        if let Some(response) = reader.feed(chunk.get(..n).unwrap_or_default())? {
            return Ok(response);
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// The status, header fields and framing of an answer.
type Head = (u16, Vec<(String, String)>, Framing);

/// How the body of an answer ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    /// After this many bytes.
    Length(usize),
    /// After the last chunk.
    Chunked,
    /// With the stream.
    Eof,
}

/// Where a chunked body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chunk {
    /// At a size line.
    Size,
    /// In the data of a chunk, with this many bytes to come.
    Data(usize),
    /// At the line break after a chunk's data.
    End,
}

/// Reads an answer as its bytes come: the header once, then the body as it arrives, so no byte
/// is looked at twice.
#[derive(Debug, Default)]
pub struct Reader {
    /// What came that isn't taken apart yet.
    pending: Vec<u8>,
    /// How far `pending` was searched for the end of the header.
    searched: usize,
    /// How many interim answers (`100 Continue`) came.
    interim: usize,
    /// The status, header fields and framing, once the header is read.
    head: Option<Head>,
    /// The body so far, without the chunk framing.
    body: Vec<u8>,
    chunk: Option<Chunk>,
}

impl Reader {
    /// Takes `bytes`; the answer once it is complete.
    ///
    /// # Errors
    ///
    /// If the answer is malformed or too large.
    pub fn feed(&mut self, bytes: &[u8]) -> io::Result<Option<Response>> {
        if self.pending.len() + self.body.len() + bytes.len() > MAX_RESPONSE + 64 * 1024 {
            return Err(invalid("CUPS's answer is too large"));
        }
        self.pending.extend_from_slice(bytes);
        while self.head.is_none() {
            // The end of the header may begin up to three bytes before what was searched.
            let from = self.searched.saturating_sub(3);
            let Some(at) = find(self.pending.get(from..).unwrap_or_default(), b"\r\n\r\n") else {
                self.searched = self.pending.len();
                if self.pending.len() > 64 * 1024 {
                    return Err(invalid("CUPS's answer has an oversized header"));
                }
                return Ok(None);
            };
            let end = from + at;
            let head = parse_head(self.pending.get(..end).unwrap_or_default())?;
            self.pending.drain(..end + 4);
            self.searched = 0;
            if (100..200).contains(&head.0) {
                // Interim answers come before the real one; a few at most.
                self.interim += 1;
                if self.interim > MAX_INTERIM {
                    return Err(invalid("CUPS sent too many interim answers"));
                }
                continue;
            }
            if head.2 == Framing::Chunked {
                self.chunk = Some(Chunk::Size);
            }
            self.head = Some(head);
        }
        self.body_step()
    }

    /// The answer at the end of the stream: a body without a length ends there.
    ///
    /// # Errors
    ///
    /// If the answer is cut off or malformed.
    pub fn finish(&mut self) -> io::Result<Option<Response>> {
        if let Some(response) = self.feed(&[])? {
            return Ok(Some(response));
        }
        match &self.head {
            None => Err(invalid("CUPS's answer has no complete header")),
            Some((status, headers, Framing::Eof)) => {
                let mut body = std::mem::take(&mut self.body);
                body.append(&mut self.pending);
                Ok(Some(Response {
                    status: *status,
                    headers: headers.clone(),
                    body,
                }))
            }
            Some(_) => Err(invalid("CUPS's answer is cut off")),
        }
    }

    /// Moves what came into the body; the answer once the body is complete.
    fn body_step(&mut self) -> io::Result<Option<Response>> {
        let Some(framing) = self.head.as_ref().map(|(_, _, framing)| *framing) else {
            return Ok(None);
        };
        let done = match framing {
            Framing::Length(length) => {
                let wanted = length.saturating_sub(self.body.len());
                let take = wanted.min(self.pending.len());
                self.body.extend(self.pending.drain(..take));
                self.body.len() == length
            }
            Framing::Eof => {
                self.body.append(&mut self.pending);
                if self.body.len() > MAX_RESPONSE {
                    return Err(invalid("CUPS's answer is too large"));
                }
                false
            }
            Framing::Chunked => self.dechunk()?,
        };
        if !done {
            return Ok(None);
        }
        Ok(self.head.as_ref().map(|(status, headers, _)| Response {
            status: *status,
            headers: headers.clone(),
            body: std::mem::take(&mut self.body),
        }))
    }

    /// Takes the chunks that came apart; true after the last one.
    fn dechunk(&mut self) -> io::Result<bool> {
        loop {
            match self.chunk {
                Some(Chunk::Size) => {
                    let Some(line_end) = find(&self.pending, b"\r\n") else {
                        if self.pending.len() > 1024 {
                            return Err(invalid("bad chunk size"));
                        }
                        return Ok(false);
                    };
                    let line =
                        String::from_utf8_lossy(self.pending.get(..line_end).unwrap_or_default())
                            .into_owned();
                    self.pending.drain(..line_end + 2);
                    let size = line.split(';').next().unwrap_or_default().trim();
                    let size =
                        usize::from_str_radix(size, 16).map_err(|_| invalid("bad chunk size"))?;
                    if size > MAX_RESPONSE || self.body.len() + size > MAX_RESPONSE {
                        return Err(invalid("CUPS's answer is too large"));
                    }
                    if size == 0 {
                        // The trailer isn't needed: the connection closes after the answer.
                        return Ok(true);
                    }
                    self.chunk = Some(Chunk::Data(size));
                }
                Some(Chunk::Data(left)) => {
                    let take = left.min(self.pending.len());
                    self.body.extend(self.pending.drain(..take));
                    if take < left {
                        self.chunk = Some(Chunk::Data(left - take));
                        return Ok(false);
                    }
                    self.chunk = Some(Chunk::End);
                }
                Some(Chunk::End) => {
                    if self.pending.len() < 2 {
                        return Ok(false);
                    }
                    if self.pending.get(..2) != Some(b"\r\n") {
                        return Err(invalid("bad chunk end"));
                    }
                    self.pending.drain(..2);
                    self.chunk = Some(Chunk::Size);
                }
                None => return Ok(false),
            }
        }
    }
}

/// The status, header fields and framing of a header (without its blank line).
fn parse_head(head: &[u8]) -> io::Result<Head> {
    let head = String::from_utf8_lossy(head);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(invalid("CUPS's answer isn't HTTP/1"));
    }
    let status: u16 = parts
        .next()
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| invalid("CUPS's answer has no status"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let chunked = headers.iter().any(|(name, value)| {
        name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
    });
    let mut length: Option<usize> = None;
    for (_, value) in headers.iter().filter(|(name, _)| name == "content-length") {
        let value: usize = value
            .parse()
            .map_err(|_| invalid("CUPS's answer has a bad length"))?;
        if length.is_some_and(|length| length != value) {
            return Err(invalid("CUPS's answer has two different lengths"));
        }
        length = Some(value);
    }
    let framing = if chunked {
        Framing::Chunked
    } else if let Some(length) = length {
        if length > MAX_RESPONSE {
            return Err(invalid("CUPS's answer is too large"));
        }
        Framing::Length(length)
    } else if (200..300).contains(&status) {
        Framing::Eof
    } else {
        // An error without a body length has no body worth waiting for.
        Framing::Length(0)
    };
    Ok((status, headers, framing))
}

#[cfg(test)]
/// The answer in `buffer`, fed at once; `None` while more is to come. At the end of the stream
/// (`eof`), a body without a length ends there.
///
/// # Errors
///
/// If the answer is malformed or too large.
fn parse(buffer: &[u8], eof: bool) -> io::Result<Option<Response>> {
    let mut reader = Reader::default();
    match reader.feed(buffer)? {
        Some(response) => Ok(Some(response)),
        None if eof => reader.finish(),
        None => Ok(None),
    }
}

/// Where `needle` first is in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_interface_is_at_the_port_in_use() {
        assert_eq!(
            Endpoint::Socket("/run/cups/cups.sock".into()).web_interface(),
            "http://localhost:631/"
        );
        assert_eq!(
            Endpoint::Loopback(Loopback::V6, 8631).web_interface(),
            "http://[::1]:8631/"
        );
        assert_eq!(
            Endpoint::Loopback(Loopback::Localhost, 632).web_interface(),
            "http://localhost:632/"
        );
    }

    #[test]
    fn a_request_has_the_ipp_headers() {
        let bytes = request("/admin/", b"IPP", Some("PeerCred alice"));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("POST /admin/ HTTP/1.1\r\nHost: localhost\r\n"));
        assert!(text.contains("Content-Type: application/ipp\r\nContent-Length: 3\r\n"));
        assert!(text.contains("Authorization: PeerCred alice\r\n"));
        assert!(text.ends_with("\r\n\r\nIPP"));
    }

    #[test]
    fn answers_with_a_length_are_read_when_complete() {
        let answer =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: 4\r\n\r\nab";
        assert_eq!(parse(answer, false).unwrap(), None);
        assert!(parse(answer, true).is_err());
        let full = [answer.as_slice(), b"cd"].concat();
        let response = parse(&full, false).unwrap().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"abcd");
        assert_eq!(response.header("content-type"), Some("application/ipp"));
    }

    #[test]
    fn chunked_answers_are_joined() {
        let answer =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n";
        assert_eq!(parse(answer, false).unwrap(), None);
        let full = [answer.as_slice(), b"0\r\n\r\n"].concat();
        assert_eq!(parse(&full, false).unwrap().unwrap().body, b"abcde");
    }

    #[test]
    fn interim_answers_are_skipped_and_others_end_with_the_stream() {
        let answer = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"CUPS\", PeerCred\r\n\r\n";
        // An error without a length has no body to wait for.
        let response = parse(answer, false).unwrap().unwrap();
        assert_eq!(response.status, 401);
        assert_eq!(
            response.header("www-authenticate"),
            Some("Basic realm=\"CUPS\", PeerCred")
        );
    }

    #[test]
    fn an_answer_fed_byte_by_byte_is_read_once_complete() {
        let answer = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        let mut reader = Reader::default();
        let mut response = None;
        for (i, byte) in answer.iter().enumerate() {
            let got = reader.feed(&[*byte]).unwrap();
            if got.is_some() {
                assert!(i >= answer.len() - 5, "complete too early at {i}");
                response = got;
                break;
            }
        }
        assert_eq!(response.unwrap().body, b"abcde");
        let mut reader = Reader::default();
        assert_eq!(
            reader.feed(b"HTTP/1.1 200 OK\r\n\r\nall of it").unwrap(),
            None
        );
        assert_eq!(reader.finish().unwrap().unwrap().body, b"all of it");
    }

    #[test]
    fn interim_answers_and_lengths_are_limited() {
        let interim = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(100_000);
        assert!(parse(&interim, false).is_err());
        let two = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\nabc";
        assert!(parse(two, false).is_err());
        let same = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\nab";
        assert_eq!(parse(same, false).unwrap().unwrap().body, b"ab");
        let mut reader = Reader::default();
        assert!(reader.feed(&vec![b'a'; MAX_RESPONSE + 128 * 1024]).is_err());
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse(b"SSH-2.0\r\n\r\n", false).is_err());
        assert!(parse(b"HTTP/1.1 OK\r\n\r\n", false).is_err());
        assert!(
            parse(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
                false
            )
            .is_err()
        );
        assert!(
            parse(
                b"HTTP/1.1 200 OK\r\nContent-Length: 99999999999\r\n\r\n",
                false
            )
            .is_err()
        );
    }
}
