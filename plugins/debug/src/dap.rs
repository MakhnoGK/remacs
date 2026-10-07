//! A connection to a debug adapter: Debug Adapter Protocol messages, each a JSON body
//! after a `Content-Length` header, over the adapter's stdio or a TCP socket. Reading and
//! writing happen on threads of their own.

use std::cell::Cell;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::Notifier;
use serde_json::{Value, json};

/// How long an adapter that listens on a port has to start listening.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_RETRY: Duration = Duration::from_millis(50);

pub enum Incoming {
    Message(Value),
    /// What the adapter wrote besides messages, e.g. to stderr.
    Log(String),
    /// The adapter went away, or could not be reached.
    Closed(Option<String>),
}

pub struct Connection {
    outgoing: Sender<Vec<u8>>,
    incoming: Receiver<Incoming>,
    next_seq: Cell<i64>,
    /// The adapter, when this connection started it; killed with the connection.
    adapter: Option<Child>,
}

impl Connection {
    /// Starts `program` and talks to it over its stdin and stdout.
    pub fn stdio(
        program: &Path,
        args: &[String],
        cwd: &Path,
        notifier: Notifier,
    ) -> io::Result<Self> {
        let mut adapter = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = adapter.stdin.take().expect("piped");
        let stdout = adapter.stdout.take().expect("piped");
        let stderr = adapter.stderr.take().expect("piped");
        let (send, incoming) = mpsc::channel();
        forward_log(stderr, send.clone(), notifier.clone());
        spawn_reader(stdout, send, notifier);
        Ok(Self::new(spawn_writer(stdin), incoming, Some(adapter)))
    }

    /// Talks to an adapter listening on `port`, waiting for it to start listening;
    /// `adapter` is the process listening, if this connection started it.
    pub fn tcp(port: u16, adapter: Option<Child>, notifier: Notifier) -> Self {
        let (send, incoming) = mpsc::channel();
        let mut adapter = adapter;
        if let Some(adapter) = &mut adapter {
            if let Some(stdout) = adapter.stdout.take() {
                forward_log(stdout, send.clone(), notifier.clone());
            }
            if let Some(stderr) = adapter.stderr.take() {
                forward_log(stderr, send.clone(), notifier.clone());
            }
        }
        let (outgoing, queued) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("dap-connect".into())
            .spawn(move || {
                let stream = match connect(port) {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = send.send(Incoming::Closed(Some(error)));
                        notifier();
                        return;
                    }
                };
                match stream.try_clone() {
                    Ok(reader) => spawn_reader(reader, send, notifier),
                    Err(error) => {
                        let _ = send.send(Incoming::Closed(Some(error.to_string())));
                        notifier();
                        return;
                    }
                }
                write_all(stream, &queued);
            })
            .expect("spawning a connecting thread");
        Self::new(outgoing, incoming, adapter)
    }

    fn new(
        outgoing: Sender<Vec<u8>>,
        incoming: Receiver<Incoming>,
        adapter: Option<Child>,
    ) -> Self {
        Self {
            outgoing,
            incoming,
            next_seq: Cell::new(1),
            adapter,
        }
    }

    /// Sends a request; its response carries the returned `seq` as `request_seq`.
    pub fn request(&self, command: &str, arguments: Value) -> i64 {
        let seq = self.take_seq();
        self.send(&json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        }));
        seq
    }

    /// Answers a request the adapter made, like `startDebugging`.
    pub fn respond(&self, request: &Value, result: Result<Value, String>) {
        let seq = self.take_seq();
        let mut response = json!({
            "seq": seq,
            "type": "response",
            "request_seq": request["seq"],
            "command": request["command"],
            "success": result.is_ok(),
        });
        match result {
            Ok(body) => response["body"] = body,
            Err(message) => response["message"] = message.into(),
        }
        self.send(&response);
    }

    fn take_seq(&self) -> i64 {
        let seq = self.next_seq.get();
        self.next_seq.set(seq + 1);
        seq
    }

    fn send(&self, message: &Value) {
        let _ = self.outgoing.send(encode(message));
    }

    pub fn try_receive(&self) -> Option<Incoming> {
        self.incoming.try_recv().ok()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(mut adapter) = self.adapter.take() {
            let _ = adapter.kill();
            thread::spawn(move || adapter.wait());
        }
    }
}

fn connect(port: u16) -> Result<TcpStream, String> {
    let start = Instant::now();
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return Ok(stream),
            Err(error) if start.elapsed() > CONNECT_TIMEOUT => {
                return Err(format!("could not connect to the debug adapter: {error}"));
            }
            Err(_) => thread::sleep(CONNECT_RETRY),
        }
    }
}

/// A port no one listens on now, for an adapter to listen on.
pub fn free_port() -> io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()?
        .port())
}

pub fn encode(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// The next message, or none at the end of the stream.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end();
        if header.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length.unwrap_or_default()];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn spawn_reader(stream: impl Read + Send + 'static, send: Sender<Incoming>, notifier: Notifier) {
    thread::Builder::new()
        .name("dap-read".into())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            let closed = loop {
                match read_message(&mut reader) {
                    Ok(Some(message)) => {
                        if send.send(Incoming::Message(message)).is_err() {
                            return;
                        }
                        notifier();
                    }
                    Ok(None) => break None,
                    Err(error) => break Some(error.to_string()),
                }
            };
            let _ = send.send(Incoming::Closed(closed));
            notifier();
        })
        .expect("spawning a reading thread");
}

fn spawn_writer(stream: impl Write + Send + 'static) -> Sender<Vec<u8>> {
    let (outgoing, queued) = mpsc::channel();
    thread::Builder::new()
        .name("dap-write".into())
        .spawn(move || write_all(stream, &queued))
        .expect("spawning a writing thread");
    outgoing
}

fn write_all(mut stream: impl Write, queued: &Receiver<Vec<u8>>) {
    for bytes in queued {
        if stream
            .write_all(&bytes)
            .and_then(|()| stream.flush())
            .is_err()
        {
            return;
        }
    }
}

fn forward_log(stream: impl Read + Send + 'static, send: Sender<Incoming>, notifier: Notifier) {
    thread::Builder::new()
        .name("dap-log".into())
        .spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else {
                    return;
                };
                if send.send(Incoming::Log(line)).is_err() {
                    return;
                }
                notifier();
            }
        })
        .expect("spawning a log thread");
}
