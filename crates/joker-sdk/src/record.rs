//! Records, record fields and enums (ref/04 §4.2 derives, §4.10): the
//! deterministic CBOR of `Internal<T>`, events, mailbox messages, action
//! arguments and persisted state, as a map with integer keys counted from 0.

use core::time::Duration;

use crate::bounded::{Bits, BoundedBytes, BoundedString, BoundedVec};
use crate::cbor::{self, Reader, Writer};
use crate::time::{ClockQuality, Instant, MonoNanos, UtcNanos, UtcSeconds};
use crate::value::{Quality, QualityCode};

/// A type a record field can have (ref/04 §4.2): scalars, time types,
/// `Quality`, enums, `Option<T>`, records, `BoundedVec<T, N ≤ 64>` and
/// `BoundedString<N ≤ 1024>`. The derives implement it; anything else is
/// `J-E0721`.
#[diagnostic::on_unimplemented(
    message = "J-E0721: `{Self}` is not a record field type",
    label = "not a record field type",
    note = "a field is a scalar, Instant, MonoNanos, UtcNanos, UtcSeconds, Duration, Quality, an enum or record with a joker derive, Option<T>, BoundedVec<T, N <= 64> or BoundedString<N <= 1024>"
)]
pub trait Field: Sized {
    /// The `field-type` of `joker:meta` (`spec/cddl/joker-meta.cddl`).
    const TYPE: u8;
    /// The most bytes the encoded value takes.
    const MAX_LEN: usize;
    /// Writes the value.
    fn encode(&self, w: &mut Writer<'_>);
    /// Reads a value.
    fn decode(r: &mut Reader<'_>) -> Option<Self>;
    /// True when the field is left out of its record (`Option::None`).
    fn omitted(&self) -> bool {
        false
    }
    /// The value of a field missing from an encoded record, if it may be.
    fn absent() -> Option<Self> {
        None
    }
}

/// A record of the app: `#[derive(joker::Record)]` (ref/04 §4.10).
pub trait Record: Field + 'static {
    /// The schema name in `joker:meta`.
    const NAME: &'static str;

    /// Encodes into `buf`; `None` if it does not fit.
    fn encode_into(&self, buf: &mut [u8]) -> Option<usize> {
        let mut w = Writer::new(buf);
        self.encode(&mut w);
        w.finish()
    }

    /// Decodes a whole buffer.
    fn decode_from(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader::new(bytes);
        let v = Self::decode(&mut r)?;
        r.is_empty().then_some(v)
    }
}

/// An event record: `#[derive(joker::Event)]`.
pub trait Event: Record {}

/// An enum without data: `#[derive(joker::Enum)]`.
pub trait Enum: Copy + Eq + 'static {
    /// The numeric value of the variant.
    fn value(self) -> u64;
    /// The variant of a numeric value.
    fn from_value(v: u64) -> Option<Self>;
}

impl Field for () {
    const TYPE: u8 = 9;
    const MAX_LEN: usize = 1;
    fn encode(&self, w: &mut Writer<'_>) {
        w.map(0);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        let n = r.map()?;
        for _ in 0..n {
            r.skip()?;
            r.skip()?;
        }
        Some(())
    }
}

impl Record for () {
    const NAME: &'static str = "()";
}

impl Field for bool {
    const TYPE: u8 = 0;
    const MAX_LEN: usize = 1;
    fn encode(&self, w: &mut Writer<'_>) {
        w.bool(*self);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.bool()
    }
}

impl Field for i64 {
    const TYPE: u8 = 1;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.int(*self);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.int()
    }
}

impl Field for u64 {
    const TYPE: u8 = 2;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.uint(*self);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.uint()
    }
}

impl Field for f64 {
    const TYPE: u8 = 3;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.float(*self);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.float()
    }
}

impl Field for f32 {
    const TYPE: u8 = 4;
    const MAX_LEN: usize = 5;
    fn encode(&self, w: &mut Writer<'_>) {
        w.float(f64::from(*self));
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.float().map(|v| v as f32)
    }
}

impl<const N: usize> Field for Bits<N> {
    const TYPE: u8 = 6;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.uint(self.bits());
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.uint().map(Bits::new)
    }
}

impl<const N: usize> Field for BoundedString<N> {
    const TYPE: u8 = 7;
    const MAX_LEN: usize = cbor::header_len(N as u64) + N;
    fn encode(&self, w: &mut Writer<'_>) {
        w.text(self.as_str());
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        BoundedString::try_from(r.text()?).ok()
    }
}

impl<const N: usize> Field for BoundedBytes<N> {
    const TYPE: u8 = 8;
    const MAX_LEN: usize = cbor::header_len(N as u64) + N;
    fn encode(&self, w: &mut Writer<'_>) {
        w.bytes(self.as_slice());
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        BoundedBytes::try_from(r.bytes()?).ok()
    }
}

impl<T: Field + Copy + Default, const N: usize> Field for BoundedVec<T, N> {
    const TYPE: u8 = 10;
    const MAX_LEN: usize = cbor::header_len(N as u64) + N * T::MAX_LEN;
    fn encode(&self, w: &mut Writer<'_>) {
        w.array(self.len());
        for item in self.iter() {
            item.encode(w);
        }
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        let n = r.array()?;
        let mut v = BoundedVec::new();
        for _ in 0..n {
            v.push(T::decode(r)?).ok()?;
        }
        Some(v)
    }
}

impl<T: Field> Field for Option<T> {
    const TYPE: u8 = T::TYPE;
    const MAX_LEN: usize = T::MAX_LEN;
    fn encode(&self, w: &mut Writer<'_>) {
        if let Some(v) = self {
            v.encode(w);
        }
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        T::decode(r).map(Some)
    }
    fn omitted(&self) -> bool {
        self.is_none()
    }
    fn absent() -> Option<Self> {
        Some(None)
    }
}

impl Field for MonoNanos {
    const TYPE: u8 = 13;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.uint(self.0);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.uint().map(MonoNanos)
    }
}

impl Field for UtcNanos {
    const TYPE: u8 = 14;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.int(self.0);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.int().map(UtcNanos)
    }
}

impl Field for UtcSeconds {
    const TYPE: u8 = 16;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.int(self.0);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.int().map(UtcSeconds)
    }
}

impl Field for Duration {
    const TYPE: u8 = 15;
    const MAX_LEN: usize = 9;
    fn encode(&self, w: &mut Writer<'_>) {
        w.uint(u64::try_from(self.as_nanos()).unwrap_or(u64::MAX));
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        r.uint().map(Duration::from_nanos)
    }
}

impl Field for Instant {
    const TYPE: u8 = 11;
    const MAX_LEN: usize = 1 + 9 + 9 + 1;
    fn encode(&self, w: &mut Writer<'_>) {
        w.array(3);
        w.uint(self.mono.0);
        w.int(self.utc.0);
        w.uint(self.clock as u64);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        if r.array()? != 3 {
            return None;
        }
        Some(Instant {
            mono: MonoNanos(r.uint()?),
            utc: UtcNanos(r.int()?),
            clock: ClockQuality::from_u8(u8::try_from(r.uint()?).ok()?),
        })
    }
}

impl Field for Quality {
    const TYPE: u8 = 12;
    const MAX_LEN: usize = 1 + 1 + 2;
    fn encode(&self, w: &mut Writer<'_>) {
        w.array(2);
        w.uint(self.code as u64);
        w.uint(self.reason as u64);
    }
    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        if r.array()? != 2 {
            return None;
        }
        let code = u8::try_from(r.uint()?).ok()?;
        let reason = u8::try_from(r.uint()?).ok()?;
        (code <= QualityCode::Bad as u8).then(|| Quality::from_codes(code, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::QualityReason;

    #[test]
    fn options_are_left_out_and_missing_ones_are_none() {
        let mut buf = [0u8; 16];
        let mut w = Writer::new(&mut buf);
        let none: Option<f64> = None;
        assert!(none.omitted());
        Some(2.5f64).encode(&mut w);
        let n = w.finish().unwrap();
        assert_eq!(
            Option::<f64>::decode(&mut Reader::new(&buf[..n])),
            Some(Some(2.5))
        );
        assert_eq!(Option::<f64>::absent(), Some(None));
        assert_eq!(f64::absent(), None);
    }

    #[test]
    fn time_and_quality_round_trip() {
        let i = Instant {
            mono: MonoNanos(7),
            utc: UtcNanos(-3),
            clock: ClockQuality::Holdover,
        };
        let mut buf = [0u8; 32];
        let mut w = Writer::new(&mut buf);
        i.encode(&mut w);
        Quality::bad(QualityReason::CommFailure).encode(&mut w);
        UtcSeconds(1_790_244_000).encode(&mut w);
        let n = w.finish().unwrap();
        let mut r = Reader::new(&buf[..n]);
        assert_eq!(Instant::decode(&mut r), Some(i));
        assert_eq!(
            Quality::decode(&mut r),
            Some(Quality::bad(QualityReason::CommFailure))
        );
        assert_eq!(UtcSeconds::decode(&mut r), Some(UtcSeconds(1_790_244_000)));
        assert!(r.is_empty());
        assert_eq!(<() as Record>::decode_from(&[0xa0]), Some(()));
    }
}
