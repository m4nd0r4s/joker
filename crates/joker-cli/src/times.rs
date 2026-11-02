//! The time options of `jk` (ADR 0104): durations, RFC 3339 instants,
//! `now`, `-<duration>`, and local date-times and times of day, converted
//! with the client's time zone before they reach the API (ref/09 §3.7).

use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;

/// One bound of a range, in the form the API takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// The query value: an RFC 3339 instant in UTC, `now` or `-<duration>`.
    pub query: String,
    /// The local date of the bound, for a later time of day.
    pub date: Option<Date>,
}

/// The range parameters of a query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Range {
    /// `since`, a duration.
    pub since: Option<String>,
    /// `from`.
    pub from: Option<String>,
    /// `to`.
    pub to: Option<String>,
}

impl Range {
    /// The `name=value` pairs of the range.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        [
            ("since", &self.since),
            ("from", &self.from),
            ("to", &self.to),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.clone().map(|v| (k, v)))
        .collect()
    }
}

/// Reads one instant: `now`, `-<duration>` and RFC 3339 pass as they are;
/// `YYYY-MM-DD HH:MM[:SS]` is local; `HH:MM[:SS]` is local on `date`, or
/// on `today` without one. A local time that a daylight-saving change makes
/// ambiguous or skips takes the earlier instant.
pub fn instant(
    text: &str,
    date: Option<Date>,
    tz: &TimeZone,
    today: Date,
) -> Result<Bound, String> {
    let text = text.trim();
    if text == "now" || text.starts_with('-') {
        return Ok(Bound {
            query: text.to_owned(),
            date: None,
        });
    }
    if let Ok(ts) = text.parse::<Timestamp>() {
        return Ok(Bound {
            query: text.to_owned(),
            date: Some(ts.to_zoned(tz.clone()).date()),
        });
    }
    let civil = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S"]
        .iter()
        .find_map(|f| DateTime::strptime(f, text).ok())
        .or_else(|| {
            ["%H:%M:%S", "%H:%M"]
                .iter()
                .find_map(|f| Time::strptime(f, text).ok())
                .map(|t| date.unwrap_or(today).to_datetime(t))
        })
        .ok_or_else(|| {
            format!(
                "`{text}`: expected a duration, RFC 3339, `now`, `-<duration>`, `YYYY-MM-DD HH:MM[:SS]` or `HH:MM[:SS]`"
            )
        })?;
    let ts = tz
        .to_ambiguous_timestamp(civil)
        .earlier()
        .map_err(|e| format!("`{text}`: {e}"))?;
    Ok(Bound {
        query: ts.to_string(),
        date: Some(civil.date()),
    })
}

/// The range of `--since`, `--from` and `--to`: `--since` is a duration, or
/// an instant that stands for `--from`; `--since` with `--from` is an error.
pub fn range(
    since: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    tz: &TimeZone,
    today: Date,
) -> Result<Range, String> {
    let mut out = Range::default();
    let start = match (since, from) {
        (Some(_), Some(_)) => return Err("--since and --from together".into()),
        (Some(s), None) if jokerd::wire::parse_duration(s).is_ok() => {
            out.since = Some(s.to_owned());
            None
        }
        (Some(s), None) | (None, Some(s)) => {
            Some(instant(s, None, tz, today).map_err(|e| format!("--from {e}"))?)
        }
        (None, None) => None,
    };
    let date = start.as_ref().and_then(|b| b.date);
    out.from = start.map(|b| b.query);
    out.to = to
        .map(|t| instant(t, date, tz, today).map_err(|e| format!("--to {e}")))
        .transpose()?
        .map(|b| b.query);
    Ok(out)
}

/// The client's time zone and today's date in it.
pub fn local() -> (TimeZone, Date) {
    let tz = TimeZone::system();
    let today = Timestamp::now().to_zoned(tz.clone()).date();
    (tz, today)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jiff::civil::date;
    use jiff::tz::offset;

    fn lisbon_summer() -> TimeZone {
        TimeZone::fixed(offset(1))
    }

    #[test]
    fn a_local_date_time_becomes_utc() {
        let b = instant("2026-09-24 03:00", None, &lisbon_summer(), date(2026, 1, 1)).unwrap();
        assert_eq!(b.query, "2026-09-24T02:00:00Z");
        assert_eq!(b.date, Some(date(2026, 9, 24)));
    }

    #[test]
    fn a_time_of_day_takes_the_date_of_the_other_bound_or_today() {
        let r = range(
            Some("2026-09-24 03:00"),
            None,
            Some("03:30"),
            &TimeZone::UTC,
            date(2030, 1, 1),
        )
        .unwrap();
        assert_eq!(r.from.as_deref(), Some("2026-09-24T03:00:00Z"));
        assert_eq!(r.to.as_deref(), Some("2026-09-24T03:30:00Z"));
        assert_eq!(r.since, None);
        let alone = instant("10:42:07", None, &TimeZone::UTC, date(2026, 11, 2)).unwrap();
        assert_eq!(alone.query, "2026-11-02T10:42:07Z");
    }

    #[test]
    fn durations_rfc3339_now_and_relative_pass_through() {
        let r = range(
            Some("1h"),
            None,
            Some("now"),
            &TimeZone::UTC,
            date(2026, 1, 1),
        )
        .unwrap();
        assert_eq!(r.since.as_deref(), Some("1h"));
        assert_eq!(r.to.as_deref(), Some("now"));
        let r = range(
            None,
            Some("2026-09-24T03:00:00+02:00"),
            Some("-5m"),
            &TimeZone::UTC,
            date(2026, 1, 1),
        )
        .unwrap();
        assert_eq!(r.from.as_deref(), Some("2026-09-24T03:00:00+02:00"));
        assert_eq!(r.to.as_deref(), Some("-5m"));
        assert_eq!(
            r.params(),
            vec![
                ("from", "2026-09-24T03:00:00+02:00".to_owned()),
                ("to", "-5m".to_owned())
            ]
        );
    }

    #[test]
    fn since_with_from_and_unreadable_times_are_refused() {
        assert!(
            range(
                Some("1h"),
                Some("now"),
                None,
                &TimeZone::UTC,
                date(2026, 1, 1)
            )
            .is_err()
        );
        let e = range(
            None,
            Some("yesterday"),
            None,
            &TimeZone::UTC,
            date(2026, 1, 1),
        )
        .unwrap_err();
        assert!(e.starts_with("--from `yesterday`"), "{e}");
    }
}
