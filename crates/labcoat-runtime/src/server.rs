//! A minimal HTTP/1.1 server for the JSON-RPC [`Node`].
//!
//! It accepts `POST` requests with a `Content-Length` body, answers each on
//! the same connection, and keeps connections open, which is what Bitcoin
//! RPC clients expect. Every connection gets a thread; a development network
//! has a handful of clients.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::node::{Caller, Node};

/// The largest request body accepted: a few maximum-weight transactions.
const MAX_BODY: usize = 32 * 1024 * 1024;

/// Serve `node` on `listener` until a `stop` request arrives.
pub fn serve(node: Arc<Node>, listener: TcpListener) -> std::io::Result<()> {
    let address = listener.local_addr()?;
    for stream in listener.incoming() {
        if node.stopping() {
            break;
        }
        let Ok(stream) = stream else { continue };
        let node = node.clone();
        std::thread::spawn(move || {
            if let Err(error) = connection(&node, stream) {
                tracing::debug!("connection closed: {error}");
            }
            if node.stopping() {
                // Wake the accept loop so it sees the flag and returns.
                let _ = TcpStream::connect(address);
            }
        });
    }
    Ok(())
}

fn connection(node: &Node, stream: TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }
        let request_line = request_line.trim();
        if request_line.is_empty() {
            continue;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_owned();
        let version = parts.nth(1).unwrap_or("HTTP/1.1").to_owned();

        let mut length = 0usize;
        let mut caller = Caller::default();
        let mut close = version == "HTTP/1.0";
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header)? == 0 {
                return Ok(());
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let Some((name, value)) = header.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                length = value.parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("connection") {
                close = value.eq_ignore_ascii_case("close");
            } else if name.eq_ignore_ascii_case("host") {
                caller.host = Some(value.to_owned());
            } else if name.eq_ignore_ascii_case("origin") {
                caller.origin = Some(value.to_owned());
            }
        }

        let (status, body) = if length > MAX_BODY {
            close = true;
            ("413 Payload Too Large", b"{}".to_vec())
        } else {
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            match method.as_str() {
                "POST" => ("200 OK", node.handle_body_from(&body, &caller)),
                "GET" => ("200 OK", br#"{"name":"Labcoat Network"}"#.to_vec()),
                _ => ("405 Method Not Allowed", b"{}".to_vec()),
            }
        };
        let close = close || node.stopping();
        write!(
            writer,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
            body.len(),
            if close { "close" } else { "keep-alive" }
        )?;
        writer.write_all(&body)?;
        writer.flush()?;
        if close {
            return Ok(());
        }
    }
}
