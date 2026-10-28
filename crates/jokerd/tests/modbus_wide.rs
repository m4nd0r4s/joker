//! `string` and `bytes` Modbus points (ref/03 §2.2.7, ADR 0062): `len`
//! registers read with FC 3, decoded into the signal's wide slot by the
//! real `modbus-tcp` driver over an in-memory server.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::VecDeque;
use std::sync::Arc;
use std::task::{Context, Poll};

use joker_driver_modbus::{ModbusTcp, ModbusTcpPlanner};
use joker_hal::link::TcpEndpoint;
use joker_hal::testing::VirtualHost;
use joker_hal::{Driver, IoError, Link, OpenArgs, Planner, TcpConnect, TcpIo, TcpStream};
use joker_manifest::OsFileSystem;
use jokerd::drivers::{point_targets, run_driver};
use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::shared::Shared;

/// Holding registers 0–3 hold `"JK01"` then `0xBEEF 0x0042`.
const HOLDING: [u16; 4] = [0x4A4B, 0x3031, 0xBEEF, 0x0042];

struct RegsIo {
    reply: VecDeque<u8>,
}

impl TcpIo for RegsIo {
    fn poll_read(&mut self, _cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        if self.reply.is_empty() {
            return Poll::Pending;
        }
        let n = buf.len().min(self.reply.len());
        for (b, r) in buf.iter_mut().zip(self.reply.drain(..n)) {
            *b = r;
        }
        Poll::Ready(Ok(n))
    }

    fn poll_write(&mut self, _cx: &mut Context<'_>, frame: &[u8]) -> Poll<Result<usize, IoError>> {
        let pdu = &frame[7..];
        let start = usize::from(u16::from_be_bytes([pdu[1], pdu[2]]));
        let count = usize::from(u16::from_be_bytes([pdu[3], pdu[4]]));
        let reply: Vec<u8> = match pdu[0] {
            3 if start + count <= HOLDING.len() => {
                let mut out = vec![3, (2 * count) as u8];
                for w in &HOLDING[start..start + count] {
                    out.extend_from_slice(&w.to_be_bytes());
                }
                out
            }
            fc => vec![fc | 0x80, 2],
        };
        self.reply.extend(&frame[..4]);
        self.reply
            .extend(u16::try_from(reply.len() + 1).unwrap().to_be_bytes());
        self.reply.push_back(frame[6]);
        self.reply.extend(reply);
        Poll::Ready(Ok(frame.len()))
    }

    fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        Poll::Ready(Ok(()))
    }
}

struct RegsServer;

impl TcpConnect for RegsServer {
    fn poll_connect(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, IoError>> {
        Poll::Ready(Ok(TcpStream::new(RegsIo {
            reply: VecDeque::new(),
        })))
    }
}

const PROFILE: &str = r#"
schema = 1

[profile]
name    = "acme/meter"
version = "1.0.0"
driver  = "modbus"

[signals.model]
at   = 0
fc   = 3
type = "string"
len  = 2

[signals.raw]
at   = 2
fc   = 3
type = "bytes"
len  = 2
"#;

#[test]
fn string_and_bytes_points_read_len_registers_into_their_wide_slots() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("joker.toml"),
        "[project]\nname = \"w\"\njoker = \"1.0\"\nmode = \"dev\"\n\n\
         [devices.m]\ndriver = \"modbus-tcp\"\naddress = \"127.0.0.1:502\"\nunit_id = 1\n\
         profile = \"acme/meter@1\"\npoll = \"20ms\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
    std::fs::write(
        dir.path().join("profiles/acme/meter@1.profile.toml"),
        PROFILE,
    )
    .unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let plan = &loaded.plan;
    let shared = Arc::new(Shared::new(plan));

    let owned = jokerd::bridge::owned_request(&plan.devices[0]);
    let views = owned.views();
    let hal_plan = ModbusTcpPlanner::plan(&views.request()).unwrap();
    let driver = ModbusTcp::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Tcp(TcpEndpoint::new(RegsServer)),
    )
    .unwrap();
    let targets = point_targets(plan, 0);
    let host = Arc::new(VirtualHost::new(0));
    let clock = Arc::clone(&host);
    host.run(run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        plan.devices[0].timeout_ns,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || clock.mono() >= 100_000_000,
    ))
    .unwrap();

    let wide = |path: &str| {
        let sample = shared
            .wide_slot(plan.id(path).unwrap())
            .unwrap()
            .read(8)
            .unwrap();
        assert!(sample.quality().is_good(), "{path}: {:?}", sample.quality());
        sample.as_slice().to_vec()
    };
    assert_eq!(wide("m.model"), b"JK01");
    assert_eq!(wide("m.raw"), [0xBE, 0xEF, 0x00, 0x42]);
}
