//! Read-only discovery (`POST /v1/discovery`, `jk discover`, docs/05 §7,
//! ref/09 §5.17, ADR 0095): the SunSpec walk of ADR 0077 against one piece
//! of equipment reached by an equipment URI (ref/03 §2.2.20), with FC 3
//! reads only and a rate limit. The result is what the walk found, the
//! `[discovered.<device>]` entry it would pin and the profile it suggests.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use joker_driver_modbus::sunspec::{self, Discovery, Found, ReadError, Step, Walk};
use joker_hal::BatchFailure;
use serde_json::{Value, json};

/// A refused request: the code and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// `J-E1708` or `J-E1012`.
    pub code: &'static str,
    /// Why.
    pub message: String,
}

fn refused(code: &'static str, message: impl Into<String>) -> Refused {
    Refused {
        code,
        message: message.into(),
    }
}

/// Where the equipment is (ref/03 §2.2.20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The host: a DNS name, an IPv4, or an IPv6 without its brackets.
    pub host: String,
    /// The TCP port.
    pub port: u16,
    /// The Modbus unit ids to walk, in order.
    pub units: Vec<u8>,
}

fn units(text: &str) -> Result<Vec<u8>, Refused> {
    let bad = || {
        refused(
            "J-E1708",
            format!("`unit={text}`: a unit is 0–255, or a range `a..b`"),
        )
    };
    let parse = |t: &str| t.parse::<u8>().map_err(|_| bad());
    match text.split_once("..") {
        Some((a, b)) => {
            let (a, b) = (parse(a)?, parse(b)?);
            if a > b {
                return Err(bad());
            }
            Ok((a..=b).collect())
        }
        None => Ok(vec![parse(text)?]),
    }
}

/// Parses the URI of a discovery (ref/03 §2.2.20). Only `sunspec://` walks
/// a device in this runtime; `modbus-tcp://` and the other schemes need a
/// driver that scans (ref/06 §2.11), and are refused with `J-E1708`.
pub fn parse_uri(uri: &str, unit: Option<&str>) -> Result<Target, Refused> {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return Err(refused(
            "J-E1708",
            format!("`{uri}` is not an equipment URI"),
        ));
    };
    match scheme {
        "sunspec" => {}
        "modbus-tcp" | "modbus-rtu" | "opcua" => {
            return Err(refused(
                "J-E1708",
                format!(
                    "`{scheme}`: the driver does not scan in this runtime; `sunspec://<host>` walks one device"
                ),
            ));
        }
        other => return Err(refused("J-E1708", format!("unknown scheme `{other}`"))),
    }
    let (authority, query) = rest.split_once('?').unwrap_or((rest, ""));
    if authority.contains('@') {
        return Err(refused(
            "J-E1708",
            "an equipment URI never carries credentials",
        ));
    }
    let authority = authority.trim_end_matches('/');
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6
            .split_once(']')
            .ok_or_else(|| refused("J-E1708", format!("`{authority}`: unclosed `[`")))?;
        (host, after.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if host.is_empty() || host.contains('/') {
        return Err(refused(
            "J-E1708",
            format!("`{authority}`: sunspec:// takes one host, not a network"),
        ));
    }
    let port = match port {
        Some(p) => p
            .parse::<u16>()
            .map_err(|_| refused("J-E1708", format!("`{p}` is not a port")))?,
        None => 502,
    };
    let mut from_query = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        match pair.split_once('=') {
            Some(("unit", v)) => from_query = Some(v.to_owned()),
            _ => {
                return Err(refused(
                    "J-E1708",
                    format!("`{pair}`: sunspec:// takes only `unit`"),
                ));
            }
        }
    }
    let units = match (from_query.as_deref(), unit) {
        (Some(a), Some(b)) if a != b => {
            return Err(refused(
                "J-E1708",
                format!("`?unit={a}` and `unit = {b}` disagree"),
            ));
        }
        (Some(u), _) | (None, Some(u)) => units(u)?,
        (None, None) => vec![1],
    };
    Ok(Target {
        host: host.to_owned(),
        port,
        units,
    })
}

/// Parses `rate` (`"10/s"`, `"5"`): requests per second, 1–1000.
pub fn parse_rate(text: Option<&str>) -> Result<u32, Refused> {
    let Some(text) = text else {
        return Ok(10);
    };
    text.trim()
        .trim_end_matches("/s")
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|r| (1..=1000).contains(r))
        .ok_or_else(|| {
            refused(
                "J-E1012",
                format!("`rate = {text}`: 1 to 1000 requests per second"),
            )
        })
}

/// A Modbus TCP client that reads holding registers, one request at a
/// time, no faster than its rate.
struct Client {
    stream: TcpStream,
    transaction: u16,
    gap: Duration,
    last: Option<Instant>,
    deadline: Instant,
}

impl Client {
    fn read(&mut self, unit: u8, at: u16, count: u16) -> Result<Vec<u16>, ReadError> {
        if Instant::now() >= self.deadline {
            return Err(ReadError::Failed(BatchFailure::Timeout));
        }
        if let Some(last) = self.last {
            let wait = self.gap.saturating_sub(last.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        self.last = Some(Instant::now());
        self.transaction = self.transaction.wrapping_add(1);
        let t = self.transaction.to_be_bytes();
        let a = at.to_be_bytes();
        let c = count.to_be_bytes();
        let request = [t[0], t[1], 0, 0, 0, 6, unit, 3, a[0], a[1], c[0], c[1]];
        let failed = |e: std::io::Error| {
            ReadError::Failed(match e.kind() {
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                    BatchFailure::Timeout
                }
                _ => BatchFailure::CommFailure("the TCP connection failed"),
            })
        };
        self.stream.write_all(&request).map_err(failed)?;
        let mut header = [0u8; 8];
        self.stream.read_exact(&mut header).map_err(failed)?;
        let length = usize::from(u16::from_be_bytes([header[4], header[5]]));
        if header[0..2] != t || length < 2 {
            return Err(ReadError::Failed(BatchFailure::CommFailure(
                "a Modbus answer that does not match the request",
            )));
        }
        let mut body = vec![0u8; length - 2];
        self.stream.read_exact(&mut body).map_err(failed)?;
        if header[7] == 0x83 {
            return Err(ReadError::Exception);
        }
        let expected = usize::from(count) * 2;
        if header[7] != 3 || body.first().map(|b| usize::from(*b)) != Some(expected) {
            return Err(ReadError::Failed(BatchFailure::CommFailure(
                "a Modbus answer that does not match the request",
            )));
        }
        Ok(body[1..]
            .chunks(2)
            .map(|w| u16::from_be_bytes([w[0], w.get(1).copied().unwrap_or(0)]))
            .collect())
    }
}

fn walk(client: &mut Client, unit: u8) -> Result<Found, sunspec::DiscoveryError> {
    let discovery = Discovery::default();
    let mut walk = Walk::new(&discovery);
    loop {
        match walk.step() {
            Step::Read { at, count } => {
                let answer = client.read(unit, at, count);
                walk.feed(answer.as_deref().map_err(|e| *e));
            }
            Step::Done => return Ok(walk.found()),
            Step::Failed(e) => return Err(e),
        }
    }
}

/// The name a suggested profile gets: `<vendor>/<model>`, in lowercase
/// with hyphens, or `local/sunspec` without an identity.
fn profile_name(found: &Found) -> String {
    let slug = |t: &str| {
        let mut out = String::new();
        for c in t.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c.to_ascii_lowercase());
            } else if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_end_matches('-').to_owned()
    };
    match found.identity() {
        Some(id) if !slug(&id.vendor).is_empty() && !slug(&id.model).is_empty() => {
            format!("{}/{}", slug(&id.vendor), slug(&id.model))
        }
        _ => "local/sunspec".to_owned(),
    }
}

/// The result of one unit that answered the walk.
fn hit(target: &Target, unit: u8, found: &Found, at: &str) -> Value {
    let identity = found.identity().unwrap_or_default();
    let name = profile_name(found);
    let models: Vec<Value> = found
        .chain
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "at": m.at,
                "len": m.len,
                "known": sunspec::definition(m.id).is_some(),
                "accepted": found.models.contains(m),
            })
        })
        .collect();
    let device = name
        .rsplit('/')
        .next()
        .unwrap_or("sunspec")
        .replace('-', "_");
    json!({
        "address": format!("{}:{}", target.host, target.port),
        "unit": unit,
        "identity": {
            "vendor": identity.vendor,
            "model": identity.model,
            "serial": identity.serial,
            "firmware": identity.firmware,
        },
        "base": found.base,
        "models": models,
        "digest": found.digest(),
        "profile_name": format!("{name}@1"),
        "profile": sunspec::profile(found, &name, &Default::default()),
        "lock": found.lock_entry(&device, "modbus-tcp", &format!("{name}@1"), at),
    })
}

/// Runs a discovery of `target` at `rate` requests per second until
/// `timeout`, and returns its `DiscoveryResult` (ADR 0095). `now` is the
/// RFC 3339 instant written in the lock entries.
pub fn run(uri: &str, target: &Target, rate: u32, timeout: Duration, now: &str) -> Value {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut hits = Vec::new();
    let mut misses = Vec::new();
    let address = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next());
    let Some(address) = address else {
        return json!({
            "uri": uri,
            "hits": [],
            "misses": [{"unit": null, "code": "J0876", "message": format!("`{}` does not resolve", target.host)}],
        });
    };
    for &unit in &target.units {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            misses
                .push(json!({"unit": unit, "code": "J0876", "message": "the discovery timed out"}));
            continue;
        }
        let stream = match TcpStream::connect_timeout(&address, left.min(Duration::from_secs(5))) {
            Ok(s) => s,
            Err(e) => {
                misses.push(json!({"unit": unit, "code": "J0876", "message": format!("cannot connect to {address}: {e}")}));
                continue;
            }
        };
        let read_timeout = Some(left.min(Duration::from_secs(3)));
        if stream.set_read_timeout(read_timeout).is_err()
            || stream.set_write_timeout(read_timeout).is_err()
        {
            misses.push(
                json!({"unit": unit, "code": "J0876", "message": "cannot set the socket timeouts"}),
            );
            continue;
        }
        let mut client = Client {
            stream,
            transaction: 0,
            gap: Duration::from_secs(1) / rate,
            last: None,
            deadline,
        };
        match walk(&mut client, unit) {
            Ok(found) => hits.push(hit(target, unit, &found, now)),
            Err(e) => {
                misses.push(json!({"unit": unit, "code": "J0876", "message": e.message()}));
            }
        }
    }
    json!({"uri": uri, "hits": hits, "misses": misses})
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn sunspec_uris_parse_and_others_are_refused() {
        let t = parse_uri("sunspec://192.168.10.30", None).unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.units),
            ("192.168.10.30", 502, vec![1])
        );
        let t = parse_uri("sunspec://[fe80::1]:1502?unit=3..5", None).unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.units),
            ("fe80::1", 1502, vec![3, 4, 5])
        );
        assert_eq!(
            parse_uri("sunspec://inv.local", Some("7")).unwrap().units,
            vec![7]
        );
        for bad in [
            "sunspec://user@inv.local",
            "sunspec://inv.local?password=x",
            "sunspec://192.168.10.0/24",
            "modbus-tcp://192.168.10.0/24",
            "ftp://x",
            "sunspec://inv.local:99999",
        ] {
            assert_eq!(parse_uri(bad, None).unwrap_err().code, "J-E1708", "{bad}");
        }
        assert!(parse_uri("sunspec://inv.local?unit=2", Some("3")).is_err());
        assert_eq!(parse_rate(None).unwrap(), 10);
        assert_eq!(parse_rate(Some("20/s")).unwrap(), 20);
        assert!(parse_rate(Some("0")).is_err());
    }
}
