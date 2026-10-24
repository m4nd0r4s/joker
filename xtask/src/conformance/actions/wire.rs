//! The wire double of the `action` suites (ADR 0073): the same
//! `[[double]]` declaration served as a Modbus TCP server on loopback
//! (application protocol v1.1b3 §4 and §6), so the steps of an action go
//! through the real `modbus-tcp` driver and reach these registers over a
//! socket.
//!
//! Every address holds one value. FC 1 and 2 read it as a bit (non-zero),
//! FC 3 and 4 as a 16-bit register; FC 5, 6, 15 and 16 write it, record
//! the write and apply `on_write`. A write of several registers is
//! recorded once, at its first address, with the registers joined in
//! order. `fail` maps onto the wire: `device_fault` is exception 04,
//! `timeout` is no answer, `comm_failure` an answer to another
//! transaction. `quality` and `native` have no form on the wire.

use std::sync::{MutexGuard, PoisonError};

use joker_hal::types::BatchFailure;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::double::{Reg, Shared, State};

fn locked(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn word(pdu: &[u8], at: usize) -> u16 {
    match pdu.get(at..at + 2) {
        Some(&[a, b]) => u16::from_be_bytes([a, b]),
        _ => 0,
    }
}

/// What the double does with one request.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Pdu(Vec<u8>),
    Garbled(Vec<u8>),
    Silent,
}

fn write(st: &mut State, at: i64, value: Reg) {
    st.writes.push((at, value));
    st.registers.insert(at, value);
    let effects: Vec<(i64, Reg)> = st
        .spec
        .on_write
        .iter()
        .filter(|e| e.at == at && e.value.same(value))
        .flat_map(|e| e.set.iter().copied())
        .collect();
    st.registers.extend(effects);
}

fn serve(state: &Shared, pdu: &[u8]) -> Answer {
    let Some(&fc) = pdu.first() else {
        return Answer::Silent;
    };
    let start = i64::from(word(pdu, 1));
    let n = word(pdu, 3);
    let mut st = locked(state);
    let span = match fc {
        1..=4 | 15 | 16 => i64::from(n.max(1)),
        _ => 1,
    };
    let failure = (start..start + span).find_map(|at| st.spec.fail.get(&at).copied());
    let ok = pdu.get(..5).map(<[u8]>::to_vec).unwrap_or_default();
    let answer = match fc {
        1 | 2 => {
            let mut data = vec![0u8; usize::from(n.div_ceil(8))];
            for i in 0..usize::from(n) {
                let on = st
                    .registers
                    .get(&(start + i as i64))
                    .is_some_and(|r| r.as_i64() != 0);
                if on {
                    data[i / 8] |= 1 << (i % 8);
                }
            }
            let mut out = vec![fc, data.len() as u8];
            out.extend(data);
            out
        }
        3 | 4 => {
            let mut out = vec![fc, (2 * n) as u8];
            for i in 0..i64::from(n) {
                let r = st.registers.get(&(start + i)).map_or(0, |r| r.as_i64());
                out.extend_from_slice(&(r as u16).to_be_bytes());
            }
            out
        }
        5 if failure.is_none() => {
            write(&mut st, start, Reg::Bool(n == 0xFF00));
            ok
        }
        6 if failure.is_none() => {
            write(&mut st, start, Reg::Int(i64::from(n)));
            ok
        }
        15 if failure.is_none() => {
            for i in 0..usize::from(n) {
                let on = pdu.get(6 + i / 8).is_some_and(|b| b >> (i % 8) & 1 == 1);
                write(&mut st, start + i as i64, Reg::Bool(on));
            }
            ok
        }
        16 if failure.is_none() => {
            let mut joined = 0i64;
            for i in 0..usize::from(n) {
                let w = word(pdu, 6 + 2 * i);
                joined = joined.wrapping_mul(65_536).wrapping_add(i64::from(w));
                st.registers
                    .insert(start + i as i64, Reg::Int(i64::from(w)));
            }
            write(&mut st, start, Reg::Int(joined));
            ok
        }
        5 | 6 | 15 | 16 => ok,
        _ => vec![fc | 0x80, 1],
    };
    match failure {
        None => Answer::Pdu(answer),
        Some(BatchFailure::Timeout) => Answer::Silent,
        Some(BatchFailure::CommFailure(_)) => Answer::Garbled(answer),
        Some(_) => Answer::Pdu(vec![fc | 0x80, 4]),
    }
}

async fn session(mut stream: tokio::net::TcpStream, state: Shared) {
    let mut header = [0u8; 7];
    while stream.read_exact(&mut header).await.is_ok() {
        let len = usize::from(u16::from_be_bytes([header[4], header[5]]));
        let mut pdu = vec![0u8; len.saturating_sub(1)];
        if stream.read_exact(&mut pdu).await.is_err() {
            return;
        }
        let (transaction, reply) = match serve(&state, &pdu) {
            Answer::Silent => continue,
            Answer::Pdu(p) => (u16::from_be_bytes([header[0], header[1]]), p),
            Answer::Garbled(p) => (
                u16::from_be_bytes([header[0], header[1]]).wrapping_add(1),
                p,
            ),
        };
        let mut out = transaction.to_be_bytes().to_vec();
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(reply.len() as u16 + 1).to_be_bytes());
        out.push(header[6]);
        out.extend(reply);
        if stream.write_all(&out).await.is_err() {
            return;
        }
    }
}

/// Serves `state` on `listener` until the runtime stops.
pub async fn serve_on(listener: std::net::TcpListener, state: Shared) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(session(stream, state.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::double::{Effect, Spec, shared};
    use super::*;

    #[test]
    fn writes_are_recorded_and_raise_their_effects() {
        let state = shared(&Spec {
            on_write: vec![Effect {
                at: 40310,
                value: Reg::Int(1),
                set: vec![(30010, Reg::Int(1))],
            }],
            ..Spec::default()
        });
        let [a0, a1] = 40310u16.to_be_bytes();
        assert_eq!(
            serve(&state, &[6, a0, a1, 0, 1]),
            Answer::Pdu(vec![6, a0, a1, 0, 1])
        );
        let [r0, r1] = 30010u16.to_be_bytes();
        assert_eq!(
            serve(&state, &[4, r0, r1, 0, 1]),
            Answer::Pdu(vec![4, 2, 0, 1])
        );
        assert_eq!(
            serve(&state, &[16, 0, 1, 0, 2, 4, 0, 1, 0, 2]),
            Answer::Pdu(vec![16, 0, 1, 0, 2])
        );
        let st = locked(&state);
        assert_eq!(st.writes, [(40310, Reg::Int(1)), (1, Reg::Int(65_538))]);
        assert_eq!(st.registers.get(&2), Some(&Reg::Int(2)));
    }

    #[test]
    fn failures_take_their_wire_form() {
        let mut spec = Spec::default();
        spec.fail.insert(1, BatchFailure::DeviceFault("x"));
        spec.fail.insert(2, BatchFailure::Timeout);
        spec.fail.insert(3, BatchFailure::CommFailure("x"));
        let state = shared(&spec);
        assert_eq!(serve(&state, &[6, 0, 1, 0, 1]), Answer::Pdu(vec![0x86, 4]));
        assert_eq!(serve(&state, &[3, 0, 2, 0, 1]), Answer::Silent);
        assert!(matches!(
            serve(&state, &[3, 0, 3, 0, 1]),
            Answer::Garbled(_)
        ));
        assert!(locked(&state).writes.is_empty());
    }
}
