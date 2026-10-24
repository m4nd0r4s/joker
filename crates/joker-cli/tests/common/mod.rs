//! Shared helpers for `jk` integration tests that need a real `jokerd`
//! subprocess on the other end of the socket.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// `jokerd`'s binary, built alongside this test but in a sibling crate:
/// `CARGO_BIN_EXE_jokerd` is only set for binaries of *this* package.
/// The workspace's target directory is a fixed property of the
/// workspace, not of where this test's own binary happens to land (this
/// environment's cargo layout places test binaries outside the usual
/// `target/debug/deps/`), so it is found from the workspace root
/// instead: `CARGO_MANIFEST_DIR` is `<root>/crates/joker-cli`.
fn jokerd_bin() -> PathBuf {
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target")
                .to_owned()
        });
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    target_dir.join(profile).join(if cfg!(windows) {
        "jokerd.exe"
    } else {
        "jokerd"
    })
}

/// A plain `sim`-only manifest: one bool output. No app writes it, so
/// it lives in the implicit `default` group (docs/16 A11) — there is
/// no `[cycles.*]` here on purpose, since nothing would use it.
pub const MANIFEST: &str = "[project]\n\
     name  = \"cli-test\"\n\
     joker = \"1.0\"\n\
     mode  = \"dev\"\n\
     \n\
     [outputs.\"pi.act_led\"]\n\
     safe = false\n\
     \n\
     [devices.pi]\n\
     driver = \"sim\"\n\
     \n\
     [devices.pi.signals]\n\
     act_led = { type = \"bool\", direction = \"output\" }\n";

pub struct Daemon {
    child: Child,
    pub socket: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts a real `jokerd` over `dir`, with [`MANIFEST`], and waits for
/// its socket to come up.
pub fn spawn_jokerd(dir: &Path) -> Daemon {
    std::fs::write(dir.join("joker.toml"), MANIFEST).unwrap();
    let socket = dir.join("api.sock");
    let child = Command::new(jokerd_bin())
        .args([
            "--state-dir",
            dir.to_str().unwrap(),
            "--manifest",
            dir.join("joker.toml").to_str().unwrap(),
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_socket(child, socket)
}

/// Starts a real `jokerd` over the state directory `dir` with no entry
/// manifest, as a node waits for its first `jk apply`, and waits for its
/// socket to come up.
pub fn spawn_empty_jokerd(dir: &Path) -> Daemon {
    let socket = dir.join("api.sock");
    let child = Command::new(jokerd_bin())
        .args([
            "--state-dir",
            dir.to_str().unwrap(),
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    wait_for_socket(child, socket)
}

/// How long a test waits for a spawned `jokerd` to accept connections: a
/// debug build on a loaded CI runner can take several seconds to start.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(30);

/// Polls until `jokerd` accepts a connection on `socket`, so that a test
/// never races the daemon's startup. Panics if the daemon exits first or
/// the socket does not come up within [`SOCKET_TIMEOUT`].
fn wait_for_socket(mut child: Child, socket: PathBuf) -> Daemon {
    let deadline = Instant::now() + SOCKET_TIMEOUT;
    loop {
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return Daemon { child, socket };
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("jokerd exited before its socket came up: {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "jokerd did not accept connections on {} within {:?}",
                socket.display(),
                SOCKET_TIMEOUT
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Stops a `jokerd` the way systemd does (`SIGTERM`) and waits for it.
pub fn terminate(mut daemon: Daemon) {
    let _ = Command::new("kill")
        .args(["-TERM", &daemon.child.id().to_string()])
        .status();
    let _ = daemon.child.wait();
}

/// Spawns `jk` with `args`, collects everything it prints to stdout
/// until `window` elapses, then kills it. For commands that run
/// forever on purpose (`--watch`, `jk top`).
pub fn run_and_collect(args: &[&str], window: Duration) -> String {
    let mut jk = Command::new(env!("CARGO_BIN_EXE_jk"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = jk.stdout.take().unwrap();
    let collected = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let mut out = BufReader::new(stdout);
        let mut buf = String::new();
        let mut line = String::new();
        while out.read_line(&mut line).unwrap_or(0) > 0 {
            buf.push_str(&line);
            line.clear();
        }
        buf
    });
    std::thread::sleep(window);
    let _ = jk.kill();
    let _ = jk.wait();
    collected.join().unwrap_or_default()
}
