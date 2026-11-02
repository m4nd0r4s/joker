//! The minimal diff of a generation swap (docs/01 §15, ADR 0103), against
//! Modbus TCP servers on loopback: a generation that changes only an
//! envelope keeps the driver's connection (the server sees no new
//! connection and the device's reconnect counter stays at zero), while a
//! generation that changes the device's address restarts that driver.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use joker_core::QualityCode;
use jokerd::catalog::CatalogFiles;
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};
use jokerd::real_devices::RealDevices;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PROFILE_PATH: &str = "profiles/acme/pcs@1.profile.toml";

const PROFILE: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[signals.p_ac]
at   = 0
fc   = 4
type = "i16"
unit = "kW"

[signals.p_set]
at        = 10
fc        = 6
type      = "i16"
unit      = "kW"
direction = "output"
"#;

/// A Modbus TCP server that answers every read with 7 and every write
/// with its echo, counting the connections it accepts and the requests it
/// serves.
#[derive(Debug, Default)]
struct Counts {
    accepted: AtomicU64,
    requests: AtomicU64,
}

async fn serve(listener: tokio::net::TcpListener, counts: Arc<Counts>) {
    while let Ok((mut stream, _)) = listener.accept().await {
        counts.accepted.fetch_add(1, Ordering::SeqCst);
        let counts = Arc::clone(&counts);
        tokio::spawn(async move {
            let mut header = [0u8; 7];
            while stream.read_exact(&mut header).await.is_ok() {
                let len = usize::from(u16::from_be_bytes([header[4], header[5]]));
                let mut pdu = vec![0u8; len.saturating_sub(1)];
                if stream.read_exact(&mut pdu).await.is_err() {
                    return;
                }
                counts.requests.fetch_add(1, Ordering::SeqCst);
                let reply = match pdu[0] {
                    3 | 4 => {
                        let n = u16::from_be_bytes([pdu[3], pdu[4]]);
                        let mut out = vec![pdu[0], (2 * n) as u8];
                        for _ in 0..n {
                            out.extend_from_slice(&7u16.to_be_bytes());
                        }
                        out
                    }
                    6 | 16 => pdu[..5].to_vec(),
                    fc => vec![fc | 0x80, 1],
                };
                let mut out = header[..4].to_vec();
                out.extend_from_slice(&(reply.len() as u16 + 1).to_be_bytes());
                out.push(header[6]);
                out.extend(reply);
                if stream.write_all(&out).await.is_err() {
                    return;
                }
            }
        });
    }
}

fn server(runtime: &tokio::runtime::Runtime) -> (u16, Arc<Counts>) {
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let counts = Arc::new(Counts::default());
    runtime.spawn(serve(listener, Arc::clone(&counts)));
    (port, counts)
}

fn manifest(port: u16, max_kw: u32) -> String {
    format!(
        r#"
[project]
name  = "minimal-diff"
joker = "1.0"
mode  = "dev"

[cycles.main]
period = "20ms"

[devices.pcs]
driver  = "modbus-tcp"
address = "127.0.0.1:{port}"
unit_id = 1
profile = "acme/pcs@1"
poll    = "50ms"
timeout = "500ms"

[outputs."pcs.p_set"]
safe                    = "0 kW"
range                   = ["0 kW", "{max_kw} kW"]
accept_missing_fallback = "bench inverter, no load behind it"
"#
    )
}

fn files() -> CatalogFiles {
    [(PROFILE_PATH.to_owned(), PROFILE.to_owned())]
        .into_iter()
        .collect()
}

fn controller(dir: &std::path::Path, runtime: &tokio::runtime::Runtime) -> Controller {
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
    ));
    let clock = Monotonic::start();
    Controller::new(
        dir,
        log,
        Arc::new(NativeTasks),
        Arc::new(RealDevices::new(runtime.handle().clone()).with_monotonic(clock)),
        Box::new(move || Box::new(SystemPacer::new(clock, joker_core::ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap()
}

fn apply(c: &mut Controller, manifest: &str) -> u64 {
    let g = c
        .propose_files(manifest, "", &files(), None, false, None)
        .unwrap();
    c.activate(g.number, None, None).unwrap();
    g.number
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The quality and the sequence of `pcs.p_ac` in the running generation,
/// and the device's (connects, reconnects).
fn reading(c: &Controller) -> (QualityCode, u64, u64, u64) {
    let (_, _, running) = c.current().unwrap();
    let id = running.plan.id("pcs.p_ac").unwrap();
    let sample = running.shared.slots[id.index()].read(8).unwrap();
    let stats = &running.shared.devices[0];
    (
        sample.quality().code(),
        sample.seq(),
        stats.connects.get(),
        stats.reconnects.get(),
    )
}

#[test]
fn an_envelope_change_keeps_the_connection_and_an_address_change_restarts_the_driver() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (port_a, a) = server(&runtime);
    let (port_b, b) = server(&runtime);
    let dir = tempfile::tempdir().unwrap();
    let mut c = controller(dir.path(), &runtime);

    apply(&mut c, &manifest(port_a, 100));
    wait_until("the first reading", || {
        reading(&c).0 == QualityCode::Good && a.requests.load(Ordering::SeqCst) > 0
    });
    assert_eq!(a.accepted.load(Ordering::SeqCst), 1);

    let envelope = apply(&mut c, &manifest(port_a, 50));
    assert_eq!(c.current().unwrap().0, envelope);
    let served = a.requests.load(Ordering::SeqCst);
    wait_until("a reading in the new generation", || {
        let (quality, seq, _, _) = reading(&c);
        quality == QualityCode::Good && seq > 0 && a.requests.load(Ordering::SeqCst) > served + 2
    });
    assert_eq!(
        a.accepted.load(Ordering::SeqCst),
        1,
        "an envelope change must not reconnect the Modbus driver"
    );
    let (_, _, connects, reconnects) = reading(&c);
    assert_eq!(
        (connects, reconnects),
        (1, 0),
        "the kept driver carries its counters"
    );

    apply(&mut c, &manifest(port_b, 50));
    wait_until("the driver on the new address", || {
        b.accepted.load(Ordering::SeqCst) == 1 && reading(&c).0 == QualityCode::Good
    });
    let served = a.requests.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        a.requests.load(Ordering::SeqCst),
        served,
        "the driver on the old address was stopped"
    );
    assert_eq!(a.accepted.load(Ordering::SeqCst), 1);
    c.shutdown();
}
