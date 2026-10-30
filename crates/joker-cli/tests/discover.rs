//! `jk discover sunspec://…` against a running `jokerd` and a simulated
//! SunSpec device (docs/05 §7, ADR 0095).

#![allow(clippy::unwrap_used)]

mod common;

use std::io::{Read, Write};
use std::process::Command;

use joker_driver_modbus::sunspec::{Image, common_body};

fn server() -> std::net::SocketAddr {
    let mut image = Image::new(40000);
    image.model(1, &common_body("ACME", "PCS 125", "4.12.0", "A1B2C3"));
    image.model(103, &[0u16; 50]);
    image.end();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let mut request = [0u8; 12];
            while stream.read_exact(&mut request).is_ok() {
                let at = u16::from_be_bytes([request[8], request[9]]);
                let count = u16::from_be_bytes([request[10], request[11]]);
                let mut answer = request[0..4].to_vec();
                match image.read(at, count) {
                    Some(words) => {
                        answer.extend((3 + 2 * words.len() as u16).to_be_bytes());
                        answer.extend([request[6], 3, (2 * words.len()) as u8]);
                        for w in words {
                            answer.extend(w.to_be_bytes());
                        }
                    }
                    None => answer.extend([0, 3, request[6], 0x83, 2]),
                }
                if stream.write_all(&answer).is_err() {
                    break;
                }
            }
        }
    });
    address
}

#[test]
fn discover_walks_a_sunspec_device_and_writes_its_profile() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());
    let node = format!("unix:{}", daemon.socket.display());
    let uri = format!("sunspec://{}", server());
    let out = Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(dir.path())
        .args([
            "--node",
            &node,
            "discover",
            &uri,
            "--rate",
            "200",
            "--out",
            "profiles/acme/pcs-125@1.profile.toml",
        ])
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("identity  ACME PCS 125"), "{text}");
    assert!(text.contains("[discovered.pcs_125]"), "{text}");
    let written =
        std::fs::read_to_string(dir.path().join("profiles/acme/pcs-125@1.profile.toml")).unwrap();
    assert!(written.contains("name    = \"acme/pcs-125\""), "{written}");
    let again = Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(dir.path())
        .args([
            "--node",
            &node,
            "discover",
            &uri,
            "--out",
            "profiles/acme/pcs-125@1.profile.toml",
        ])
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    common::terminate(daemon);
}
