//! A tiny HTTP/1.1 file server on 127.0.0.1 for the client tests: fixed bodies per path, ETag
//! support, `Connection: close`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Default)]
pub struct Files(
    Arc<Mutex<HashMap<String, Vec<u8>>>>,
    Arc<Mutex<Vec<String>>>,
);

impl Files {
    pub fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.0
            .lock()
            .expect("test step")
            .insert(path.to_owned(), body.into());
    }

    /// Every request's full header block, to check what the client sends.
    pub fn requests(&self) -> Vec<String> {
        self.1.lock().expect("test step").clone()
    }
}

pub struct TestServer {
    pub base: String,
    pub files: Files,
}

impl TestServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test step");
        let base = format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().expect("test step").port()
        );
        let files = Files::default();
        let served = files.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let files = served.clone();
                thread::spawn(move || serve(stream, &files));
            }
        });
        Self { base, files }
    }
}

/// A base URL nothing listens on.
pub fn dead_base() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test step");
    let port = listener.local_addr().expect("test step").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/")
}

fn serve(stream: TcpStream, files: &Files) {
    let mut reader = BufReader::new(stream.try_clone().expect("test step"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let path = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
    let mut if_none_match = None;
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        headers.push_str(&line);
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("if-none-match")
        {
            if_none_match = Some(value.trim().to_owned());
        }
    }
    files.1.lock().expect("test step").push(headers);
    let body = files.0.lock().expect("test step").get(&path).cloned();
    let mut stream = stream;
    let response = match body {
        None => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
        Some(body) => {
            let etag = format!("\"{}\"", crate::Digest::of(&body).sha256_hex());
            if if_none_match.as_deref() == Some(etag.as_str()) {
                format!("HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nConnection: close\r\n\r\n")
                    .into_bytes()
            } else {
                let mut out = format!(
                    "HTTP/1.1 200 OK\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                out.extend_from_slice(&body);
                out
            }
        }
    };
    let _ = stream.write_all(&response);
}
