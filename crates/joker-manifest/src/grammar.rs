//! The value grammars of ref/01 §3 that do not need units: names, paths,
//! selectors, levels, budgets, sizes and references.

use joker_units::Duration;

use crate::model::{Band, Budget, Level, SchemaVersion};

/// `Nome`: `[a-z][a-z0-9_]*`, up to 32 characters (docs/16 P5).
pub fn is_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    text.len() <= 32
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `NomeDns`: a lowercase DNS label, up to 63 characters (ref/00 §5).
pub fn is_dns_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
}

/// A driver name: `[a-z][a-z0-9-]{0,31}` (ref/06 §2.2).
pub fn is_driver_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    text.len() <= 32
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A path: up to 8 segments, each a [`is_name`] (ref/01 §3.8).
pub fn is_path(text: &str) -> bool {
    let segments: Vec<&str> = text.split('.').collect();
    segments.len() <= 8 && segments.iter().all(|s| is_name(s))
}

/// A selector: segments of `[a-z0-9_*]`, starting with a letter or `*`, or
/// `**` as a whole segment (ref/01 §3.9).
pub fn is_selector(text: &str) -> bool {
    let segments: Vec<&str> = text.split('.').collect();
    segments.len() <= 8
        && segments.iter().all(|s| {
            *s == "**"
                || (!s.contains("**")
                    && s.bytes()
                        .next()
                        .is_some_and(|b| b.is_ascii_lowercase() || b == b'*')
                    && s.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'*'
                    }))
        })
}

/// Whether a selector or path has wildcards.
pub fn is_wildcard(text: &str) -> bool {
    text.contains('*')
}

/// Whether a selector matches a path (ref/01 §3.9).
pub fn matches(selector: &str, path: &str) -> bool {
    let pattern: Vec<&str> = selector.split('.').collect();
    let segments: Vec<&str> = path.split('.').collect();
    match_segments(&pattern, &segments)
}

fn match_segments(pattern: &[&str], segments: &[&str]) -> bool {
    match (pattern.split_first(), segments.split_first()) {
        (None, None) => true,
        (Some((&"**", rest)), Some(_)) => {
            (1..=segments.len()).any(|n| match_segments(rest, &segments[n..]))
        }
        (Some((p, rest)), Some((s, tail))) => match_segment(p, s) && match_segments(rest, tail),
        _ => false,
    }
}

fn match_segment(pattern: &str, segment: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == segment,
        Some((head, tail)) => segment.strip_prefix(head).is_some_and(|rest| {
            (0..=rest.len()).any(|i| rest.is_char_boundary(i) && match_segment(tail, &rest[i..]))
        }),
    }
}

/// `"MAJOR.MINOR"`.
pub fn schema_version(text: &str) -> Option<SchemaVersion> {
    let (major, minor) = text.split_once('.')?;
    let number = |s: &str| -> Option<u16> {
        (!s.is_empty()
            && s.bytes().all(|b| b.is_ascii_digit())
            && (s == "0" || !s.starts_with('0')))
        .then(|| s.parse().ok())
        .flatten()
    };
    Some(SchemaVersion {
        major: number(major)?,
        minor: number(minor)?,
    })
}

/// A level in either notation (ref/01 §3.11); the priority defaults to the
/// lowest of the band.
pub fn level(text: &str) -> Result<Level, LevelError> {
    let (band, priority) = match text.split_once(':') {
        Some((band, priority)) => (band, Some(priority)),
        None => (text, None),
    };
    let band = Band::ALL
        .into_iter()
        .find(|b| b.as_str() == band)
        .ok_or(LevelError::Band)?;
    let priority = match priority {
        Some(p) => p.parse::<u8>().map_err(|_| LevelError::Band)?,
        None => *band.priorities().end(),
    };
    if band.priorities().contains(&priority) {
        Ok(Level { band, priority })
    } else {
        Err(LevelError::Priority(band))
    }
}

/// Why a level is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelError {
    /// Not a band or a band with a malformed priority (`J0250`).
    Band,
    /// A priority outside the band (`J0251`).
    Priority(Band),
}

/// An actuation budget `"<n>/<period>"`, period between 1 s and 7 d
/// (ref/01 §3.4).
pub fn budget(text: &str) -> Option<Budget> {
    let (count, period) = text.split_once('/')?;
    let count: u32 = count.parse().ok().filter(|c| *c >= 1)?;
    let digits = period.bytes().take_while(u8::is_ascii_digit).count();
    let window = if digits == 0 {
        Duration::parse(&format!("1{period}")).ok()?
    } else {
        Duration::parse(period).ok()?
    };
    let range = Duration::from_secs(1).as_nanos()..=Duration::from_secs(7 * 86_400).as_nanos();
    range
        .contains(&window.as_nanos())
        .then_some(Budget { count, window })
}

/// A memory or disk size with an IEC prefix (ref/01 §3.5), in bytes.
pub fn size(text: &str) -> Option<u64> {
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let factor: u64 = match unit.trim_start_matches(' ') {
        "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return None,
    };
    let value: f64 = number.parse().ok()?;
    let bytes = value * factor as f64;
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(bytes as u64)
}

/// `"<host>:<port>"` with a DNS name or an IP and no wildcards (ref/01 §5.12).
pub fn host_port(text: &str) -> bool {
    let Some((host, port)) = text.rsplit_once(':') else {
        return false;
    };
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let port_ok = port.parse::<u16>().is_ok_and(|p| p > 0);
    let host_ok = host.parse::<std::net::IpAddr>().is_ok()
        || (!host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && !label.starts_with('-')
                    && !label.ends_with('-')
            }));
    port_ok && host_ok
}

/// `"sha256:<64 hex>"`.
pub fn digest(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// `"oci://<registry>/<repo>:<tag>"` or `"oci://<registry>/<repo>@sha256:<hex>"`.
pub fn oci_ref(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("oci://") else {
        return false;
    };
    let Some((registry, repo)) = rest.split_once('/') else {
        return false;
    };
    let reference_ok = match repo.split_once('@') {
        Some((name, digest_part)) => !name.is_empty() && digest(digest_part),
        None => repo
            .rsplit_once(':')
            .is_some_and(|(name, tag)| !name.is_empty() && !tag.is_empty() && !tag.contains('/')),
    };
    !registry.is_empty() && reference_ok
}

/// A profile reference `"<vendor>/<model>@<major>"`; `sunspec` is the short
/// form of `joker/sunspec@1` (ref/01 §5.8). `None` without a major.
pub fn profile_ref(text: &str) -> Option<String> {
    if text == "sunspec" {
        return Some("joker/sunspec@1".to_owned());
    }
    let (name, major) = text.split_once('@')?;
    let (vendor, model) = name.split_once('/')?;
    let part = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    };
    (part(vendor) && part(model) && major.parse::<u32>().is_ok()).then(|| text.to_owned())
}

/// The edit distance between two keys, for "did you mean" suggestions.
pub fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = if ca == *cb {
                previous
            } else {
                1 + previous.min(row[j]).min(current)
            };
            previous = current;
        }
    }
    row[b.len()]
}

/// The closest candidate within two edits, if any.
pub fn suggest<'a>(key: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .into_iter()
        .map(|c| (distance(key, c), c))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert!(is_name("peak_shaver") && !is_name("Peak") && !is_name("1a") && !is_name(""));
        assert!(is_dns_name("site-lisboa-01") && !is_dns_name("-a") && !is_dns_name("A"));
        assert!(is_path("pcs.p_set") && !is_path("pcs..p") && !is_path("a.b.c.d.e.f.g.h.i"));
        assert!(is_driver_name("modbus-tcp") && !is_driver_name("Modbus"));
    }

    #[test]
    fn selectors_match_as_specified() {
        assert!(is_selector("inv*.ac.p") && is_selector("meter.**") && is_selector("**"));
        assert!(!is_selector("**alarm.**") && !is_selector("a..b"));
        assert!(matches("inv*.ac.p", "inv12.ac.p") && !matches("inv*.ac.p", "inv1.dc.p"));
        assert!(matches("meter.**", "meter.l1.v") && !matches("meter.**", "meter"));
        assert!(matches("p1.acc.*", "p1.acc.x") && !matches("p1.acc.*", "p1.acc.x.y"));
        assert!(matches("vav_3*", "vav_30") && matches("**", "a.b"));
    }

    #[test]
    fn levels_budgets_sizes() {
        assert_eq!(
            level("control"),
            Ok(Level {
                band: Band::Control,
                priority: 12
            })
        );
        assert_eq!(
            level("optimizer:13"),
            Ok(Level {
                band: Band::Optimizer,
                priority: 13
            })
        );
        assert_eq!(
            level("optimizer:3"),
            Err(LevelError::Priority(Band::Optimizer))
        );
        assert_eq!(level("boss"), Err(LevelError::Band));
        assert_eq!(budget("120/h").map(|b| b.count), Some(120));
        assert_eq!(
            budget("3/10min").map(|b| b.window.as_nanos()),
            Some(600_000_000_000)
        );
        assert!(budget("0/h").is_none() && budget("1/8d").is_none());
        assert_eq!(size("32 MiB"), Some(32 << 20));
        assert!(size("512 MB").is_none());
    }

    #[test]
    fn references() {
        assert!(host_port("api.tariffs.example.com:443") && !host_port("*.example.com:443"));
        assert!(host_port("192.168.1.60:502") && !host_port("example.com"));
        assert!(oci_ref(
            "oci://registry.example.com/joker/peak-shaver:1.4.2"
        ));
        assert!(!oci_ref("https://x/y:1"));
        assert_eq!(
            profile_ref("local/bench-esp32@1").as_deref(),
            Some("local/bench-esp32@1")
        );
        assert_eq!(profile_ref("sunspec").as_deref(), Some("joker/sunspec@1"));
        assert!(profile_ref("acme/pcs").is_none());
        assert_eq!(
            schema_version("1.0"),
            Some(SchemaVersion { major: 1, minor: 0 })
        );
        assert!(schema_version("1").is_none() && schema_version("1.01").is_none());
        assert_eq!(
            suggest("max_leas", ["max_lease", "max_manual"]),
            Some("max_lease")
        );
    }
}
