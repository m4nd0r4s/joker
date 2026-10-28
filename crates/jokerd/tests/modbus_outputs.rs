//! AUR-428, ADR 0076: the outputs of a `modbus-tcp` device reach a Modbus
//! TCP server on loopback through jokerd: the cycle loop applies them, the
//! output stage converts them to raw, the device's real io task and the
//! real driver write them with FC 6, FC 16 and, for a `bool` in a
//! register bit, an FC 3 read followed by the write of the register; every
//! write is confirmed by its echo.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant as Wall};

use joker_core::image::{Proposal, Source};
use joker_core::time::{ClockQuality, Instant};
use joker_core::{Outcome, SignalId};
use joker_manifest::OsFileSystem;
use jokerd::cycle::{RunOptions, run};
use jokerd::devices::Devices;
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::VirtualPacer;
use jokerd::real_devices::RealDevices;
use jokerd::shared::Shared;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MS: u64 = 1_000_000;

#[derive(Debug, Default)]
struct Server {
    holding: BTreeMap<u16, u16>,
    log: Vec<Vec<u8>>,
}

fn word(pdu: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([pdu[at], pdu[at + 1]])
}

impl Server {
    fn serve(&mut self, pdu: &[u8]) -> Vec<u8> {
        self.log.push(pdu.to_vec());
        let (fc, start, n) = (pdu[0], word(pdu, 1), word(pdu, 3));
        match fc {
            3 => {
                let mut out = vec![fc, (2 * n) as u8];
                for at in start..start + n {
                    out.extend_from_slice(
                        &self.holding.get(&at).copied().unwrap_or(0).to_be_bytes(),
                    );
                }
                out
            }
            6 => {
                self.holding.insert(start, n);
                pdu[..5].to_vec()
            }
            16 => {
                for i in 0..n {
                    self.holding
                        .insert(start + i, word(pdu, 6 + 2 * usize::from(i)));
                }
                pdu[..5].to_vec()
            }
            _ => vec![fc | 0x80, 1],
        }
    }

    fn writes(&self) -> Vec<Vec<u8>> {
        self.log
            .iter()
            .filter(|pdu| matches!(pdu[0], 6 | 16))
            .cloned()
            .collect()
    }
}

async fn serve_modbus(listener: tokio::net::TcpListener, server: Arc<Mutex<Server>>) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let server = server.clone();
        tokio::spawn(async move {
            let mut header = [0u8; 7];
            while stream.read_exact(&mut header).await.is_ok() {
                let len = usize::from(u16::from_be_bytes([header[4], header[5]]));
                let mut pdu = vec![0u8; len.saturating_sub(1)];
                if stream.read_exact(&mut pdu).await.is_err() {
                    return;
                }
                let reply = server.lock().unwrap().serve(&pdu);
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

const PROFILE: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[signals.p_set]
at        = 10
fc        = 6
type      = "i16"
scale     = 0.1
unit      = "kW"
direction = "output"

[signals.q_set]
at         = 11
fc         = 16
type       = "u32"
unit       = "kvar"
word_order = "little"
direction  = "output"

[signals.enable]
at        = 20
fc        = 6
bit       = 4
type      = "bool"
direction = "output"

[signals.standby]
at        = 20
fc        = 16
bit       = 0
type      = "bool"
invert    = true
direction = "output"

[signals.state]
at   = 20
fc   = 3
bit  = 4
type = "bool"
poll = "1s"
"#;

fn manifest(port: u16) -> String {
    format!(
        r#"
[project]
name  = "modbus-outputs"
joker = "1.0"
mode  = "dev"

[devices.pcs]
driver  = "modbus-tcp"
address = "127.0.0.1:{port}"
unit_id = 1
profile = "acme/pcs@1"
poll    = "1s"
timeout = "500ms"

[outputs."pcs.p_set"]
safe                    = "0 kW"
accept_missing_fallback = "bench inverter, no load behind it"

[outputs."pcs.q_set"]
safe                    = "0 kvar"
range                   = ["0 kvar", "100000 kvar"]
accept_missing_fallback = "bench inverter, no load behind it"

[outputs."pcs.enable"]
safe                    = false
accept_missing_fallback = "bench inverter, no load behind it"

[outputs."pcs.standby"]
safe                    = false
accept_missing_fallback = "bench inverter, no load behind it"
"#
    )
}

fn loaded(port: u16) -> (tempfile::TempDir, Loaded) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), manifest(port)).unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
    std::fs::write(dir.path().join("profiles/acme/pcs@1.profile.toml"), PROFILE).unwrap();
    let l = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    (dir, l)
}

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

/// Applies each `(output, bits)` through one tick of the cycle loop and
/// returns the outcome of each.
fn apply(l: &Loaded, shared: &Shared, writes: &[(&str, u64)]) -> Vec<Outcome> {
    let plan = &l.plan;
    for (path, bits) in writes {
        let p = Proposal {
            writer: plan.writer("user:manual:4").unwrap(),
            signal: plan.id(path).unwrap(),
            value: *bits,
            source: Source::Async,
            ttl_ns: 3_600_000 * MS,
            ..Proposal::EMPTY
        };
        shared.inboxes[0].push(p).unwrap();
    }
    run(
        plan,
        shared,
        vec![Vec::new()],
        &mut VirtualPacer::new(start()),
        RunOptions {
            max_ticks: Some(1),
            ..RunOptions::default()
        },
    )
    .unwrap();
    writes
        .iter()
        .map(|(path, _)| {
            let (_, meta, _) = shared
                .output(plan.id(path).unwrap())
                .unwrap()
                .read()
                .unwrap();
            Outcome::from_code(meta.outcome, meta.detail).unwrap()
        })
        .collect()
}

/// Waits until `done` holds of the server, or panics after two seconds.
fn wait_for(server: &Arc<Mutex<Server>>, done: impl Fn(&Server) -> bool) {
    let deadline = Wall::now() + Duration::from_secs(2);
    while !done(&server.lock().unwrap()) {
        assert!(
            Wall::now() < deadline,
            "the server never got there: {:?}",
            server.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The confirmation changes the io task queued, by output.
fn confirmations(shared: &Shared) -> Vec<(SignalId, bool)> {
    let mut out = Vec::new();
    while let Some(c) = shared.confirmations.iter().find_map(|q| q.pop()) {
        out.push((c.output, c.confirmed));
    }
    out
}

#[test]
fn register_and_register_bit_outputs_reach_the_server_with_their_exact_raw_values() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = Arc::new(Mutex::new(Server::default()));
    server
        .lock()
        .unwrap()
        .holding
        .insert(20, 0b1000_0000_0000_0000);
    runtime.spawn(serve_modbus(listener, server.clone()));

    let (_dir, l) = loaded(port);
    let shared = Arc::new(Shared::new(&l.plan));
    let outcomes = apply(
        &l,
        &shared,
        &[
            ("pcs.p_set", 12.34f64.to_bits()),
            ("pcs.q_set", 65_537.0f64.to_bits()),
            ("pcs.enable", 1),
            ("pcs.standby", 0),
        ],
    );
    assert!(
        outcomes.iter().all(|o| *o == Outcome::Accepted),
        "{outcomes:?}"
    );

    let plan = Arc::new(l.plan.clone());
    let set = RealDevices::new(runtime.handle().clone()).start(&plan, &shared);
    wait_for(&server, |s| {
        s.holding.get(&10) == Some(&123)
            && s.holding.get(&12) == Some(&1)
            && s.holding.get(&20) == Some(&0b1000_0000_0001_0001)
    });
    set.stop();

    let s = server.lock().unwrap();
    assert_eq!(s.holding.get(&10), Some(&123), "12.34 kW / 0.1 kW");
    assert_eq!(
        (s.holding.get(&11), s.holding.get(&12)),
        (Some(&1), Some(&1)),
        "65537 in two registers, least significant word first"
    );
    assert_eq!(
        s.holding.get(&20),
        Some(&0b1000_0000_0001_0001),
        "bit 4 set by enable, bit 0 set by the inverted standby, bit 15 kept"
    );
    let writes = s.writes();
    assert!(writes.contains(&vec![6, 0, 10, 0, 123]), "{writes:?}");
    assert!(
        writes.contains(&vec![16, 0, 11, 0, 2, 4, 0, 1, 0, 1]),
        "{writes:?}"
    );
    assert!(
        writes.iter().any(|w| w.starts_with(&[16, 0, 20, 0, 1, 2])),
        "the bit of an FC 16 output is written with FC 16: {writes:?}"
    );
    assert!(
        writes.iter().any(|w| w.starts_with(&[6, 0, 20])),
        "the bit of an FC 6 output is written with FC 6: {writes:?}"
    );
    let reads_of_20 = s
        .log
        .iter()
        .filter(|pdu| pdu[..] == [3, 0, 20, 0, 1])
        .count();
    assert!(
        reads_of_20 >= 2,
        "each bit write reads its register first: {:?}",
        s.log
    );
    drop(s);

    let refused = confirmations(&shared);
    assert!(
        refused.iter().all(|(_, confirmed)| *confirmed),
        "no output left unconfirmed: {refused:?}"
    );
    let mut failed = Vec::new();
    while let Some(ack) = shared.write_acks.pop() {
        failed.push(ack);
    }
    assert!(
        failed.is_empty(),
        "every write was acknowledged ok, its echo matched: {failed:?}"
    );
    runtime.shutdown_timeout(Duration::from_secs(2));
}

#[test]
fn a_bool_output_in_a_register_bit_is_no_longer_refused_at_plan_time() {
    let (_dir, l) = loaded(502);
    let enable = l.plan.devices[0]
        .points
        .iter()
        .find(|p| p.path == "pcs.enable")
        .unwrap();
    assert_eq!(enable.expect, joker_hal::RawKind::Regs);
    assert_eq!(enable.width, 1);
}
