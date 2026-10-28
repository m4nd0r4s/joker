//! `jokerd::logs::LogRing`: the node's structured log ring behind
//! `GET /v1/logs` (ref/09 §5.2, 16 MiB by default).

use jokerd::logs::{LogFilter, LogLevel, LogRing};

fn ring_with(entries: &[(LogLevel, &str, &str, i64)]) -> LogRing {
    let ring = LogRing::new(1 << 20);
    for &(level, source, msg, ts) in entries {
        ring.push(level, source, msg, Vec::new(), ts);
    }
    ring
}

fn messages(page: &[jokerd::logs::LogEntry]) -> Vec<&str> {
    page.iter().map(|e| e.msg.as_str()).collect()
}

#[test]
fn entries_are_numbered_and_newest_first_by_default() {
    let ring = ring_with(&[
        (LogLevel::Info, "jokerd", "a", 10),
        (LogLevel::Warn, "driver:pcs", "b", 20),
        (LogLevel::Error, "jokerd", "c", 30),
    ]);
    let (page, next) = ring.query(&LogFilter::default());
    assert_eq!(messages(&page), ["c", "b", "a"]);
    assert_eq!(page.iter().map(|e| e.seq).collect::<Vec<_>>(), [3, 2, 1]);
    assert_eq!(next, None);
}

#[test]
fn source_level_and_time_filter_the_entries() {
    let ring = ring_with(&[
        (LogLevel::Debug, "jokerd", "debug", 10),
        (LogLevel::Info, "driver:pcs", "pcs", 20),
        (LogLevel::Warn, "app:peak", "peak", 30),
        (LogLevel::Error, "jokerd", "late", 40),
    ]);
    let source = |s: &str| LogFilter {
        source: Some(s.to_owned()),
        ..LogFilter::default()
    };
    assert_eq!(messages(&ring.query(&source("driver:pcs")).0), ["pcs"]);
    let warn = LogFilter {
        min_level: LogLevel::Warn,
        ..LogFilter::default()
    };
    assert_eq!(messages(&ring.query(&warn).0), ["late", "peak"]);
    let window = LogFilter {
        from_ns: 15,
        to_ns: 30,
        ..LogFilter::default()
    };
    assert_eq!(messages(&ring.query(&window).0), ["peak", "pcs"]);
}

#[test]
fn a_page_ends_with_a_cursor_that_continues_it() {
    let ring = ring_with(&[
        (LogLevel::Info, "jokerd", "1", 1),
        (LogLevel::Info, "jokerd", "2", 2),
        (LogLevel::Info, "jokerd", "3", 3),
    ]);
    let first = LogFilter {
        limit: 2,
        ..LogFilter::default()
    };
    let (page, next) = ring.query(&first);
    assert_eq!(messages(&page), ["3", "2"]);
    let rest = LogFilter {
        limit: 2,
        cursor: Some(next.unwrap()),
        ..LogFilter::default()
    };
    let (page, next) = ring.query(&rest);
    assert_eq!(messages(&page), ["1"]);
    assert_eq!(next, None);

    let asc = LogFilter {
        limit: 2,
        ascending: true,
        ..LogFilter::default()
    };
    assert_eq!(messages(&ring.query(&asc).0), ["1", "2"]);
}

#[test]
fn the_oldest_entries_go_when_the_ring_is_full_and_are_counted() {
    let ring = LogRing::new(1024);
    for i in 0..100 {
        ring.push(
            LogLevel::Info,
            "jokerd",
            &format!("message {i:03}"),
            Vec::new(),
            i,
        );
    }
    let (page, _) = ring.query(&LogFilter {
        limit: 1000,
        ..LogFilter::default()
    });
    assert!(page.len() < 100, "{} entries kept", page.len());
    assert_eq!(page[0].msg, "message 099", "the newest is kept");
    assert_eq!(ring.dropped(), 100 - page.len() as u64);
}
