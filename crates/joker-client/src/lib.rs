//! The Rust client of the local API (ref/05 part B, ref/09).

use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The default socket of a node.
pub const DEFAULT_SOCKET: &str = "/run/joker/api.sock";

/// Where a node is (ref/02 §3.3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A Unix socket on this machine.
    Unix(PathBuf),
    /// An SSH tunnel to the node's Unix socket.
    Ssh {
        /// The SSH user.
        user: Option<String>,
        /// The host.
        host: String,
        /// The SSH port.
        port: Option<u16>,
    },
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unix(p) => write!(f, "unix:{}", p.display()),
            Self::Ssh { user, host, port } => {
                write!(f, "ssh://")?;
                if let Some(u) = user {
                    write!(f, "{u}@")?;
                }
                write!(f, "{host}")?;
                if let Some(p) = port {
                    write!(f, ":{p}")?;
                }
                Ok(())
            }
        }
    }
}

/// A client error, with the `jk` exit code it maps to (docs/07 §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    /// What went wrong.
    pub message: String,
    /// The exit code.
    pub exit: u8,
}

impl ClientError {
    fn new(exit: u8, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit,
        }
    }

    fn unreachable(message: impl Into<String>) -> Self {
        Self::new(4, message)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientError {}

/// Parses a node address: `local`, `unix:<path>`, `ssh://[user@]host[:port]`.
/// A TCP address needs mTLS, which this build does not have.
pub fn parse_address(text: &str) -> Result<Target, ClientError> {
    if text == "local" {
        return Ok(Target::Unix(PathBuf::from(DEFAULT_SOCKET)));
    }
    if let Some(path) = text.strip_prefix("unix:") {
        return Ok(Target::Unix(PathBuf::from(path)));
    }
    if let Some(rest) = text.strip_prefix("ssh://") {
        let (user, hostport) = match rest.split_once('@') {
            Some((u, h)) => (Some(u.to_owned()), h),
            None => (None, rest),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => {
                let port = p
                    .parse()
                    .map_err(|_| ClientError::new(1, format!("bad SSH port in `{text}`")))?;
                (h.to_owned(), Some(port))
            }
            _ => (hostport.to_owned(), None),
        };
        if host.is_empty() {
            return Err(ClientError::new(1, format!("no host in `{text}`")));
        }
        return Ok(Target::Ssh { user, host, port });
    }
    Err(ClientError::new(
        1,
        format!("`{text}`: TCP with mTLS is not in this build; use `ssh://` or `unix:`"),
    ))
}

/// Resolves `--node`: an address, or a context name from `[nodes]` of the
/// project's `.joker/config.toml` or the user's `config.toml` (ref/02
/// §2.3.2); a project entry replaces the user's whole.
pub fn resolve(node: &str, project: Option<&Path>) -> Result<Target, ClientError> {
    if node == "local" || node.starts_with("unix:") || node.starts_with("ssh://") {
        return parse_address(node);
    }
    let mut files = Vec::new();
    if let Some(p) = project {
        files.push(p.join(".joker/config.toml"));
    }
    if let Ok(path) = std::env::var("JK_CONFIG") {
        files.push(PathBuf::from(path));
    } else if let Ok(home) = std::env::var("HOME") {
        files.push(PathBuf::from(home).join(".config/joker/config.toml"));
    }
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let table: toml::Table = text
            .parse()
            .map_err(|e| ClientError::new(1, format!("{}: {e}", file.display())))?;
        if let Some(address) = table
            .get("nodes")
            .and_then(|n| n.get(node))
            .and_then(|n| n.get("address"))
            .and_then(toml::Value::as_str)
        {
            return parse_address(address);
        }
    }
    Err(ClientError::new(1, format!("unknown node `{node}`")))
}

/// A response.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// HTTP status.
    pub status: u16,
    /// Headers, lower-case names.
    pub headers: Vec<(String, String)>,
    /// Body.
    pub body: Vec<u8>,
}

impl Response {
    /// The body as JSON (`Null` when it is not JSON).
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    /// A header.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// True for 2xx.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The problem of an error response, rendered as `jk` shows it, with the
    /// exit code of ref/09 §9.2.
    pub fn problem(&self) -> ClientError {
        let p = self.json();
        let code = p["code"].as_str().unwrap_or("J-E1000");
        let mut text = format!(
            "error[{code}]: {}",
            p["detail"]
                .as_str()
                .or_else(|| p["title"].as_str())
                .unwrap_or("request failed")
        );
        if let Some(errors) = p["errors"].as_array() {
            for e in errors {
                if let Some(m) = e["message"].as_str().filter(|m| !m.is_empty()) {
                    text.push('\n');
                    text.push_str(m);
                }
            }
        }
        let exit = match self.status {
            422 => 2,
            401 | 403 | 409 | 412 => 3,
            503 | 504 | 429 => 4,
            _ => 1,
        };
        ClientError::new(exit, text)
    }
}

struct Tunnel {
    child: Child,
    socket: PathBuf,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// A connection to a node.
pub struct Client {
    target: Target,
    socket: PathBuf,
    timeout: Duration,
    _tunnel: Option<Tunnel>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("target", &self.target)
            .finish()
    }
}

fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

impl Client {
    /// Connects to a node; for SSH, forwards a local socket to the node's
    /// socket with OpenSSH (`ssh -L <local>:<remote>`).
    pub fn connect(target: Target, timeout: Duration) -> Result<Self, ClientError> {
        match &target {
            Target::Unix(path) => Ok(Self {
                socket: path.clone(),
                target,
                timeout,
                _tunnel: None,
            }),
            Target::Ssh { user, host, port } => {
                let socket = std::env::temp_dir().join(format!(
                    "jk-{}-{}.sock",
                    std::process::id(),
                    now_ns() % 1_000_000
                ));
                let mut cmd = Command::new("ssh");
                cmd.args([
                    "-N",
                    "-o",
                    "ExitOnForwardFailure=yes",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "StreamLocalBindUnlink=yes",
                ]);
                if let Some(p) = port {
                    cmd.args(["-p", &p.to_string()]);
                }
                cmd.arg("-L")
                    .arg(format!("{}:{DEFAULT_SOCKET}", socket.display()));
                cmd.arg(match user {
                    Some(u) => format!("{u}@{host}"),
                    None => host.clone(),
                });
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                let child = cmd
                    .spawn()
                    .map_err(|e| ClientError::unreachable(format!("ssh: {e}")))?;
                let mut tunnel = Tunnel {
                    child,
                    socket: socket.clone(),
                };
                let start = Instant::now();
                while !socket.exists() {
                    if let Ok(Some(status)) = tunnel.child.try_wait() {
                        let mut err = String::new();
                        if let Some(mut e) = tunnel.child.stderr.take() {
                            let _ = e.read_to_string(&mut err);
                        }
                        return Err(ClientError::unreachable(format!(
                            "ssh to {target} exited ({status}): {}",
                            err.trim()
                        )));
                    }
                    if start.elapsed() > timeout {
                        return Err(ClientError::unreachable(format!(
                            "ssh to {target}: no tunnel after {timeout:?}"
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(Self {
                    socket,
                    target,
                    timeout,
                    _tunnel: Some(tunnel),
                })
            }
        }
    }

    /// The node this client talks to.
    /// The local Unix socket of this connection: for [`Target::Unix`],
    /// the node's own socket; for [`Target::Ssh`], the local end of the
    /// forwarding tunnel. Either way, a plain Unix socket a caller can
    /// speak a different protocol over directly (`jk read --watch`'s
    /// WebSocket upgrade, ref/09 §6, does not go through [`Client::get`]
    /// at all).
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    /// `GET`.
    pub fn get(&self, path: &str) -> Result<Response, ClientError> {
        self.send("GET", path, &[], None)
    }

    /// `POST` with a JSON body; adds `Joker-Nonce`, `Joker-Timestamp` and
    /// an `Idempotency-Key` (ref/09 §9.3).
    pub fn post(
        &self,
        path: &str,
        body: &Value,
        headers: &[(&str, &str)],
    ) -> Result<Response, ClientError> {
        let nonce = joker_api::id::ulid(now_ns());
        let ts = humantime::format_rfc3339_nanos(std::time::SystemTime::now()).to_string();
        let key = joker_api::id::ulid(now_ns());
        let mut all: Vec<(&str, &str)> = vec![
            ("joker-nonce", &nonce),
            ("joker-timestamp", &ts),
            ("idempotency-key", &key),
        ];
        all.extend_from_slice(headers);
        self.send("POST", path, &all, Some(body))
    }

    /// `POST` with an `application/octet-stream` body (a blob upload,
    /// ref/09 §5.13); adds `Joker-Nonce` and `Joker-Timestamp`.
    pub fn post_bytes(&self, path: &str, body: &[u8]) -> Result<Response, ClientError> {
        let nonce = joker_api::id::ulid(now_ns());
        let ts = humantime::format_rfc3339_nanos(std::time::SystemTime::now()).to_string();
        let headers = [("joker-nonce", nonce.as_str()), ("joker-timestamp", &ts)];
        self.exchange(
            "POST",
            path,
            &headers,
            Some(("application/octet-stream", body)),
        )
    }

    /// Sends one request.
    pub fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Response, ClientError> {
        let payload = body.map(Value::to_string);
        self.exchange(
            method,
            path,
            headers,
            payload
                .as_deref()
                .map(|p| ("application/json", p.as_bytes())),
        )
    }

    fn exchange(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<(&str, &[u8])>,
    ) -> Result<Response, ClientError> {
        let mut stream = UnixStream::connect(&self.socket)
            .map_err(|e| ClientError::unreachable(format!("{}: {e}", self.target)))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| ClientError::unreachable(e.to_string()))?;
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nhost: joker\r\nconnection: close\r\nuser-agent: jk/{}\r\n",
            env!("CARGO_PKG_VERSION")
        );
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some((kind, bytes)) = body {
            req.push_str(&format!(
                "content-type: {kind}\r\ncontent-length: {}\r\n",
                bytes.len()
            ));
        }
        req.push_str("\r\n");
        let mut raw = req.into_bytes();
        if let Some((_, bytes)) = body {
            raw.extend_from_slice(bytes);
        }
        stream
            .write_all(&raw)
            .map_err(|e| ClientError::unreachable(format!("{}: {e}", self.target)))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| ClientError::unreachable(format!("{}: {e}", self.target)))?;
        parse(&raw).ok_or_else(|| ClientError::unreachable("malformed HTTP response"))
    }
}

fn parse(raw: &[u8]) -> Option<Response> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let status = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let mut body = raw.get(split + 4..)?.to_vec();
    if headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked"))
    {
        body = dechunk(&body)?;
    }
    Some(Response {
        status,
        headers,
        body,
    })
}

fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let pos = b.windows(2).position(|w| w == b"\r\n")?;
        let size =
            usize::from_str_radix(std::str::from_utf8(b.get(..pos)?).ok()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(b.get(pos + 2..pos + 2 + size)?);
        b = b.get(pos + 4 + size..)?;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            parse_address("local").unwrap(),
            Target::Unix(DEFAULT_SOCKET.into())
        );
        assert_eq!(
            parse_address("unix:/tmp/x.sock").unwrap(),
            Target::Unix("/tmp/x.sock".into())
        );
        assert_eq!(
            parse_address("ssh://pi@lab.local").unwrap(),
            Target::Ssh {
                user: Some("pi".into()),
                host: "lab.local".into(),
                port: None
            }
        );
        assert_eq!(
            parse_address("ssh://lab:2222").unwrap(),
            Target::Ssh {
                user: None,
                host: "lab".into(),
                port: Some(2222)
            }
        );
        assert_eq!(parse_address("lab:7443").unwrap_err().exit, 1);
        assert_eq!(
            parse_address("ssh://pi@lab.local").unwrap().to_string(),
            "ssh://pi@lab.local"
        );
    }

    #[test]
    fn contexts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".joker")).unwrap();
        std::fs::write(
            dir.path().join(".joker/config.toml"),
            "version = 1\n[nodes.lab]\naddress = \"ssh://pi@lab.local\"\n",
        )
        .unwrap();
        assert_eq!(
            resolve("lab", Some(dir.path())).unwrap().to_string(),
            "ssh://pi@lab.local"
        );
        assert_eq!(resolve("nope", Some(dir.path())).unwrap_err().exit, 1);
    }

    #[test]
    fn responses() {
        let r = parse(b"HTTP/1.1 409 Conflict\r\nContent-Type: application/problem+json\r\n\r\n{\"code\":\"J-E1105\",\"detail\":\"held\"}").unwrap();
        assert_eq!(r.status, 409);
        let e = r.problem();
        assert_eq!(e.exit, 3);
        assert_eq!(e.message, "error[J-E1105]: held");
        let c = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!(c.body, b"abcde");
    }
}
