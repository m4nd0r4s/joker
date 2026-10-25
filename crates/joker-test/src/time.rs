//! The UTC instants of a test, in RFC 3339 (ref/00 §3).

/// Days from 1970-01-01 to a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Nanoseconds since 1970 of `YYYY-MM-DDTHH:MM:SS[.fraction]Z`.
pub(crate) fn parse_utc(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z').or_else(|| text.strip_suffix('z'))?;
    let (date, time) = text.split_once(['T', 't'])?;
    let mut d = date.split('-');
    let (y, mo, da): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    if d.next().is_some() || !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    let (hms, fraction) = match time.split_once('.') {
        Some((hms, f)) => (hms, f),
        None => (time, ""),
    };
    let mut t = hms.split(':');
    let (h, mi, s): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    if t.next().is_some() || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    if fraction.len() > 9 || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let nanos: i64 = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<9}").parse().ok()?
    };
    let secs = days_from_civil(y, mo, da) * 86_400 + h * 3_600 + mi * 60 + s;
    secs.checked_mul(1_000_000_000)?.checked_add(nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_instants_parse_to_unix_nanoseconds() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_utc("2026-01-01T00:00:00Z"),
            Some(1_767_225_600_000_000_000)
        );
        assert_eq!(
            parse_utc("2000-02-29T12:30:15.25Z"),
            Some(951_827_415_250_000_000)
        );
        assert_eq!(parse_utc("2026-01-01T00:00:00+01:00"), None);
        assert_eq!(parse_utc("2026-13-01T00:00:00Z"), None);
    }
}
