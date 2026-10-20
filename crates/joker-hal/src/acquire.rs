//! The part of the acquisition stage that every tier shares (ref/06 §2.8,
//! §3.6, steps 1–2): checking what a driver delivered and settling the
//! quality of every point of a batch, plus the acquisition deadband.
//!
//! Settling is where a failed or silent read becomes `bad(comm_failure)`:
//! a batch failure applies to all its points, and a point the driver did
//! not deliver is `bad(comm_failure)` in that read. A sample that breaks
//! the rules is `J0869` and becomes `bad(config_error)`. Quality never
//! disappears: a `bad` result carries no value.

use crate::spec::PointSpec;
use crate::types::{
    Code, J0869, MAX_RAW_BYTES, QualityCode, RawSample, RawStatus, RawValue, Reason,
};

/// Checks one sample against its point (ref/06 §2.8, §2.7).
pub fn check_sample(point: &PointSpec<'_>, sample: &RawSample<'_>) -> Result<(), Code> {
    if !sample.status.is_allowed() {
        return Err(J0869);
    }
    let kind = sample.value.kind();
    match (sample.status.code, sample.value) {
        (QualityCode::Good, RawValue::None) => return Err(J0869),
        (_, RawValue::None) => return Ok(()),
        _ => {}
    }
    if kind != point.expect || sample.value.payload_len() > MAX_RAW_BYTES {
        return Err(J0869);
    }
    if let RawValue::Regs(regs) = sample.value
        && regs.len() != usize::from(point.width)
    {
        return Err(J0869);
    }
    Ok(())
}

/// Status of a point with an invalid sample.
pub const INVALID: RawStatus = RawStatus::bad(Reason::CONFIG_ERROR);
/// Status of a point of the batch that got no sample.
pub const MISSING: RawStatus = RawStatus::bad(Reason::COMM_FAILURE);

/// The acquisition deadband of one input (ref/01 §5.8.1): a numeric value
/// that moved by no more than `band` from the last published one is
/// suppressed. A band of 0 suppresses nothing, and a change of quality or
/// the first value always passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deadband {
    band: f64,
    last: Option<(f64, RawStatus)>,
}

impl Deadband {
    /// A deadband of `band` (absolute, in the value's unit).
    pub const fn new(band: f64) -> Self {
        Self {
            band: if band < 0.0 { -band } else { band },
            last: None,
        }
    }

    /// True when `value` with `status` must be published; records it if so.
    pub fn pass(&mut self, value: f64, status: RawStatus) -> bool {
        let publish = match self.last {
            None => true,
            Some((last, last_status)) => {
                last_status != status || self.band == 0.0 || (value - last).abs() > self.band
            }
        };
        if publish {
            self.last = Some((value, status));
        }
        publish
    }
}

#[cfg(feature = "alloc")]
pub use owned::{OwnedRaw, OwnedSample, SampleBuffer, Settled, settle};

#[cfg(feature = "alloc")]
mod owned {
    use super::*;
    use crate::driver::{Full, ReadBatch, SampleSink};
    use crate::spec::PlanRequest;
    use crate::types::{BatchFailure, MonoNs, PointId};
    use alloc::string::String;
    use alloc::vec::Vec;

    /// An owned raw value.
    #[derive(Clone, Debug, PartialEq)]
    pub enum OwnedRaw {
        /// No value.
        None,
        /// `bool`.
        Bool(bool),
        /// `i64`.
        Int(i64),
        /// `u64`.
        Uint(u64),
        /// `f64`.
        Float(f64),
        /// Registers.
        Regs(Vec<u16>),
        /// Bytes.
        Bytes(Vec<u8>),
        /// Text.
        Text(String),
    }

    impl OwnedRaw {
        /// A copy of `value`.
        pub fn copy(value: &RawValue<'_>) -> Self {
            match *value {
                RawValue::None => Self::None,
                RawValue::Bool(b) => Self::Bool(b),
                RawValue::Int(i) => Self::Int(i),
                RawValue::Uint(u) => Self::Uint(u),
                RawValue::Float(f) => Self::Float(f),
                RawValue::Regs(r) => Self::Regs(Vec::from(r)),
                RawValue::Bytes(b) => Self::Bytes(Vec::from(b)),
                RawValue::Text(t) => Self::Text(String::from(t)),
            }
        }

        /// The borrowed value.
        pub fn view(&self) -> RawValue<'_> {
            match self {
                Self::None => RawValue::None,
                Self::Bool(b) => RawValue::Bool(*b),
                Self::Int(i) => RawValue::Int(*i),
                Self::Uint(u) => RawValue::Uint(*u),
                Self::Float(f) => RawValue::Float(*f),
                Self::Regs(r) => RawValue::Regs(r),
                Self::Bytes(b) => RawValue::Bytes(b),
                Self::Text(t) => RawValue::Text(t),
            }
        }
    }

    /// An owned sample.
    #[derive(Clone, Debug, PartialEq)]
    pub struct OwnedSample {
        /// The point.
        pub point: PointId,
        /// The value.
        pub value: OwnedRaw,
        /// The quality.
        pub status: RawStatus,
        /// Equipment timestamp.
        pub ts_src: Option<i64>,
        /// Hardware timestamp.
        pub ts_hw: Option<MonoNs>,
    }

    impl OwnedSample {
        /// The borrowed sample.
        pub fn view(&self) -> RawSample<'_> {
            RawSample {
                point: self.point,
                value: self.value.view(),
                status: self.status,
                ts_src: self.ts_src,
                ts_hw: self.ts_hw,
            }
        }
    }

    /// A bounded sample buffer: the M0 stand-in for the device's SPSC ring.
    #[derive(Clone, Debug, PartialEq, Default)]
    pub struct SampleBuffer {
        samples: Vec<OwnedSample>,
        capacity: usize,
        dropped: u64,
    }

    impl SampleBuffer {
        /// A buffer of `capacity` samples, allocated now.
        pub fn with_capacity(capacity: usize) -> Self {
            Self {
                samples: Vec::with_capacity(capacity),
                capacity,
                dropped: 0,
            }
        }

        /// The buffered samples.
        pub fn samples(&self) -> &[OwnedSample] {
            &self.samples
        }

        /// Takes the buffered samples, keeping the allocation.
        pub fn clear(&mut self) {
            self.samples.clear();
        }

        /// Samples refused because the buffer was full.
        pub fn dropped(&self) -> u64 {
            self.dropped
        }
    }

    impl SampleSink for SampleBuffer {
        fn accept(&mut self, sample: &RawSample<'_>) -> Result<(), Full> {
            if self.samples.len() >= self.capacity {
                self.dropped += 1;
                return Err(Full);
            }
            self.samples.push(OwnedSample {
                point: sample.point,
                value: OwnedRaw::copy(&sample.value),
                status: sample.status,
                ts_src: sample.ts_src,
                ts_hw: sample.ts_hw,
            });
            Ok(())
        }
    }

    /// The settled result of one point of a batch.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Settled {
        /// The point.
        pub point: PointId,
        /// The value; `None` unless the status allows one.
        pub value: OwnedRaw,
        /// The quality.
        pub status: RawStatus,
        /// The rule broken, when the driver's sample was invalid.
        pub invalid: Option<Code>,
        /// Hardware timestamp.
        pub ts_hw: Option<MonoNs>,
    }

    /// Settles every point of `batch` from the samples of one `read` and its
    /// result (ref/06 §2.8, §3.6 steps 1–2). Samples of points outside the
    /// batch are ignored here; the caller counts them as `J0869`.
    pub fn settle(
        request: &PlanRequest<'_>,
        batch: &ReadBatch<'_>,
        samples: &[OwnedSample],
        result: Result<(), BatchFailure>,
    ) -> Vec<Settled> {
        batch
            .points
            .iter()
            .map(|&point| {
                let failed = |status, invalid| Settled {
                    point,
                    value: OwnedRaw::None,
                    status,
                    invalid,
                    ts_hw: None,
                };
                if let Err(failure) = result {
                    return failed(failure.status(), None);
                }
                let mut mine = samples.iter().filter(|s| s.point == point);
                let (Some(sample), None) = (mine.next(), mine.next()) else {
                    let twice = samples.iter().filter(|s| s.point == point).count() > 1;
                    return if twice {
                        failed(INVALID, Some(J0869))
                    } else {
                        failed(MISSING, None)
                    };
                };
                let Some(spec) = request.point(point) else {
                    return failed(INVALID, Some(J0869));
                };
                match check_sample(spec, &sample.view()) {
                    Ok(()) => Settled {
                        point,
                        value: if sample.status.code == QualityCode::Bad {
                            OwnedRaw::None
                        } else {
                            sample.value.clone()
                        },
                        status: sample.status,
                        invalid: None,
                        ts_hw: sample.ts_hw,
                    },
                    Err(code) => failed(INVALID, Some(code)),
                }
            })
            .collect()
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use crate::driver::{RawSink, ReadBatch};
    use crate::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
    use crate::types::{BatchFailure, BatchKind, RawKind};
    use alloc::vec;

    fn request() -> OwnedRequest {
        OwnedRequest {
            bus: None,
            devices: vec![OwnedDevice::new("d", vec![0xa0], 1)],
            points: vec![
                OwnedPoint::input(0, "a", RawKind::Bool, 0, "bool", vec![0xa0], 10),
                OwnedPoint::input(1, "b", RawKind::Regs, 2, "u32", vec![0xa0], 10),
                OwnedPoint::input(2, "c", RawKind::Float, 0, "f64", vec![0xa0], 10),
            ],
        }
    }

    fn batch(points: &[u32]) -> ReadBatch<'_> {
        ReadBatch {
            id: 0,
            device: 0,
            kind: BatchKind::Poll,
            points,
            private: &[],
            deadline: 0,
        }
    }

    #[test]
    fn batch_failures_and_missing_points_become_comm_failure() {
        let req = request();
        let views = req.views();
        let r = views.request();
        let mut buf = SampleBuffer::with_capacity(8);
        let mut sink = RawSink::new(&mut buf);
        sink.push(RawSample::good(0, RawValue::Bool(true))).unwrap();
        let settled = settle(&r, &batch(&[0, 1, 2]), buf.samples(), Ok(()));
        assert_eq!(settled[0].status, RawStatus::OK);
        assert_eq!(settled[0].value, OwnedRaw::Bool(true));
        assert_eq!(settled[1].status, MISSING);
        assert_eq!(settled[2].status, RawStatus::bad(Reason::COMM_FAILURE));
        let settled = settle(
            &r,
            &batch(&[0, 1, 2]),
            buf.samples(),
            Err(BatchFailure::Timeout),
        );
        assert!(
            settled
                .iter()
                .all(|s| s.status == MISSING && s.value == OwnedRaw::None)
        );
        let settled = settle(
            &r,
            &batch(&[0]),
            buf.samples(),
            Err(BatchFailure::DeviceFault("exception 2")),
        );
        assert_eq!(settled[0].status, RawStatus::bad(Reason::DEVICE_FAULT));
    }

    #[test]
    fn invalid_samples_are_j0869_and_config_error() {
        let req = request();
        let views = req.views();
        let r = views.request();
        let mut buf = SampleBuffer::with_capacity(8);
        let mut sink = RawSink::new(&mut buf);
        sink.push(RawSample::good(0, RawValue::Int(1))).unwrap();
        sink.push(RawSample::good(1, RawValue::Regs(&[1]))).unwrap();
        sink.push(RawSample::good(2, RawValue::Float(1.0))).unwrap();
        sink.push(RawSample::good(2, RawValue::Float(2.0))).unwrap();
        let settled = settle(&r, &batch(&[0, 1, 2]), buf.samples(), Ok(()));
        for s in &settled {
            assert_eq!((s.status, s.invalid), (INVALID, Some(J0869)), "{s:?}");
        }
        let mut small = SampleBuffer::with_capacity(1);
        let mut sink = RawSink::new(&mut small);
        assert!(sink.push(RawSample::good(0, RawValue::Bool(true))).is_ok());
        assert!(sink.push(RawSample::good(0, RawValue::Bool(true))).is_err());
        assert_eq!((sink.pushed(), sink.dropped()), (1, 1));
        assert_eq!(small.dropped(), 1);
    }

    #[test]
    fn a_bad_sample_never_carries_a_value() {
        let req = request();
        let views = req.views();
        let r = views.request();
        let mut buf = SampleBuffer::with_capacity(8);
        let mut sink = RawSink::new(&mut buf);
        sink.push(RawSample {
            status: RawStatus::bad(Reason::DEVICE_FAULT),
            ..RawSample::good(2, RawValue::Float(1.0))
        })
        .unwrap();
        sink.push(RawSample::failed(0, RawStatus::OK)).unwrap();
        let settled = settle(&r, &batch(&[0, 2]), buf.samples(), Ok(()));
        assert_eq!(settled[0].status, INVALID);
        assert_eq!(settled[1].status, RawStatus::bad(Reason::DEVICE_FAULT));
        assert_eq!(settled[1].value, OwnedRaw::None);
    }

    #[test]
    fn deadband_suppresses_small_moves_only() {
        let mut d = Deadband::new(5.0);
        assert!(d.pass(60.0, RawStatus::OK));
        assert!(!d.pass(64.0, RawStatus::OK));
        assert!(!d.pass(55.0, RawStatus::OK));
        assert!(d.pass(65.5, RawStatus::OK));
        assert!(d.pass(65.5, RawStatus::uncertain(Reason::STALE)));
        let mut zero = Deadband::new(0.0);
        assert!(zero.pass(1.0, RawStatus::OK));
        assert!(zero.pass(1.0, RawStatus::OK));
    }
}
