//! The read plan (ref/06 §2.6): batches of points, each one protocol
//! operation, computed by the pure `plan` half of a driver.
//!
//! A plan is flat so that it can be borrowed from flash on a Joker Micro:
//! the batch heads, then every batch's points and private bytes in one
//! array each. [`Plan`] (with `alloc`) owns the three arrays.

use crate::types::{BatchId, BatchKind, DeviceIx, PointId};

/// The fixed part of one batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BatchHead {
    /// Index of the batch, its position in the plan.
    pub id: BatchId,
    /// Device of all its points.
    pub device: DeviceIx,
    /// Kind.
    pub kind: BatchKind,
    /// Period of its points for `poll`, 0 otherwise.
    pub period_ns: u64,
    /// Bus time of one execution, for the budget (ref/06 §2.6).
    pub cost_ns: u64,
    /// Bus time of a write of one point.
    pub write_cost_ns: u64,
    /// First point in [`PlanView::points`].
    pub points_start: u32,
    /// Number of points.
    pub points_len: u32,
    /// First byte in [`PlanView::private`].
    pub private_start: u32,
    /// Number of private bytes.
    pub private_len: u32,
}

/// One batch, as `open` and the scheduler see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchSpec<'a> {
    /// The fixed part.
    pub head: BatchHead,
    /// The points, increasing.
    pub points: &'a [PointId],
    /// Driver-private bytes, at most 4 KiB.
    pub private: &'a [u8],
}

/// A borrowed plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PlanView<'a> {
    /// Batch heads, by id.
    pub batches: &'a [BatchHead],
    /// The points of every batch.
    pub points: &'a [PointId],
    /// The private bytes of every batch.
    pub private: &'a [u8],
    /// Driver-private bytes of the whole plan.
    pub plan_private: &'a [u8],
}

impl<'a> PlanView<'a> {
    /// Number of batches.
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// True for a plan without batches.
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// Batch `id`, if it exists and its ranges are inside the plan.
    pub fn batch(&self, id: BatchId) -> Option<BatchSpec<'a>> {
        let head = *self.batches.get(id as usize)?;
        let p = head.points_start as usize;
        let points = self
            .points
            .get(p..p.checked_add(head.points_len as usize)?)?;
        let q = head.private_start as usize;
        let private = self
            .private
            .get(q..q.checked_add(head.private_len as usize)?)?;
        Some(BatchSpec {
            head,
            points,
            private,
        })
    }

    /// Every batch, in id order; batches with broken ranges are skipped
    /// (validation refuses such a plan).
    pub fn iter(&self) -> impl Iterator<Item = BatchSpec<'a>> + '_ {
        (0..self.batches.len() as u32).filter_map(|id| self.batch(id))
    }
}

/// Bytes of private data allowed per batch (ref/06 §2.15).
pub const MAX_BATCH_PRIVATE: usize = 4 * 1024;
/// Bytes of private data allowed in a whole plan (ref/06 §2.15).
pub const MAX_PLAN_PRIVATE: usize = 1024 * 1024;
/// Batches allowed in a plan (ref/06 §2.15).
pub const MAX_BATCHES: usize = 16_384;

#[cfg(feature = "alloc")]
pub use owned::{Plan, validate};

#[cfg(feature = "alloc")]
mod owned {
    use super::*;
    use crate::spec::PlanRequest;
    use crate::types::{Diagnostic, Direction, J0856, Poll};
    use alloc::vec;
    use alloc::vec::Vec;

    /// An owned plan, built batch by batch.
    #[derive(Clone, Debug, PartialEq, Eq, Default)]
    pub struct Plan {
        batches: Vec<BatchHead>,
        points: Vec<PointId>,
        private: Vec<u8>,
        plan_private: Vec<u8>,
    }

    impl Plan {
        /// An empty plan.
        pub fn new() -> Self {
            Self::default()
        }

        /// Appends a batch and returns its id. `points` are sorted and
        /// deduplicated here, so that equal inputs give equal plans.
        #[allow(clippy::too_many_arguments)]
        pub fn push(
            &mut self,
            device: DeviceIx,
            kind: BatchKind,
            period_ns: u64,
            points: &[PointId],
            cost_ns: u64,
            write_cost_ns: u64,
            private: &[u8],
        ) -> BatchId {
            let id = self.batches.len() as BatchId;
            let mut sorted = Vec::from(points);
            sorted.sort_unstable();
            sorted.dedup();
            self.batches.push(BatchHead {
                id,
                device,
                kind,
                period_ns,
                cost_ns,
                write_cost_ns,
                points_start: self.points.len() as u32,
                points_len: sorted.len() as u32,
                private_start: self.private.len() as u32,
                private_len: private.len() as u32,
            });
            self.points.extend_from_slice(&sorted);
            self.private.extend_from_slice(private);
            id
        }

        /// Sets the private bytes of the whole plan.
        pub fn set_plan_private(&mut self, bytes: &[u8]) {
            self.plan_private = Vec::from(bytes);
        }

        /// The borrowed view.
        pub fn view(&self) -> PlanView<'_> {
            PlanView {
                batches: &self.batches,
                points: &self.points,
                private: &self.private,
                plan_private: &self.plan_private,
            }
        }
    }

    /// Checks a plan against its request with the rules of ref/06 §2.6;
    /// every violation is `J0856`.
    pub fn validate(
        request: &PlanRequest<'_>,
        plan: &PlanView<'_>,
    ) -> Result<(), Diagnostic<'static>> {
        let fail = |message| Err(Diagnostic::general(J0856, message));
        if plan.len() > MAX_BATCHES {
            return fail("too many batches");
        }
        if plan.plan_private.len() + plan.private.len() > MAX_PLAN_PRIVATE {
            return fail("plan private data above 1 MiB");
        }
        let mut seen = vec![false; request.points.len()];
        for (i, head) in plan.batches.iter().enumerate() {
            let Some(batch) = plan.batch(i as BatchId) else {
                return fail("batch ranges outside the plan");
            };
            if head.id as usize != i {
                return fail("batch ids are not 0..n in order");
            }
            if batch.private.len() > MAX_BATCH_PRIVATE {
                return fail("batch private data above 4 KiB");
            }
            if batch.points.is_empty() {
                return fail("empty batch");
            }
            if batch.points.windows(2).any(|w| w[0] >= w[1]) {
                return fail("batch points not increasing");
            }
            for &id in batch.points {
                let Some(index) = request.points.iter().position(|p| p.id == id) else {
                    return Err(Diagnostic::at(J0856, id, "point not in the request"));
                };
                let point = &request.points[index];
                if point.device != head.device {
                    return Err(Diagnostic::at(J0856, id, "point of another device"));
                }
                if point.direction == Direction::Output {
                    return Err(Diagnostic::at(J0856, id, "output in a batch"));
                }
                let kind_ok = match (head.kind, point.poll) {
                    (BatchKind::Poll, Poll::Period(p)) => p == head.period_ns && p > 0,
                    (BatchKind::OnDemand, Poll::OnDemand) => head.period_ns == 0,
                    (BatchKind::Push, Poll::Push) => head.period_ns == 0,
                    _ => false,
                };
                if !kind_ok {
                    return Err(Diagnostic::at(J0856, id, "batch kind or period mismatch"));
                }
                if seen[index] {
                    return Err(Diagnostic::at(J0856, id, "point in two batches"));
                }
                seen[index] = true;
            }
        }
        for (point, covered) in request.points.iter().zip(&seen) {
            if point.direction != Direction::Output && !covered {
                return Err(Diagnostic::at(J0856, point.id, "point in no batch"));
            }
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use crate::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
    use crate::types::{BatchKind, J0856, RawKind};
    use alloc::vec;

    fn request() -> OwnedRequest {
        OwnedRequest {
            bus: None,
            devices: vec![OwnedDevice::new("d", vec![0xa0], 1_000_000_000)],
            points: vec![
                OwnedPoint::input(0, "a", RawKind::Bool, 0, "bool", vec![0xa0], 100),
                OwnedPoint::input(1, "b", RawKind::Bool, 0, "bool", vec![0xa0], 100),
                OwnedPoint::output(2, "c", RawKind::Bool, 0, "bool", vec![0xa0]),
                OwnedPoint::input(3, "d", RawKind::Bool, 0, "bool", vec![0xa0], 200),
            ],
        }
    }

    #[test]
    fn a_complete_plan_validates_and_views_back() {
        let req = request();
        let views = req.views();
        let mut plan = Plan::new();
        plan.push(0, BatchKind::Poll, 100, &[1, 0, 1], 5, 0, &[7]);
        plan.push(0, BatchKind::Poll, 200, &[3], 5, 0, &[]);
        let view = plan.view();
        assert_eq!(validate(&views.request(), &view), Ok(()));
        let b = view.batch(0).unwrap();
        assert_eq!(b.points, &[0, 1]);
        assert_eq!(b.private, &[7]);
        assert_eq!(view.iter().count(), 2);
        assert_eq!(view.batch(2), None);
    }

    #[test]
    fn plan_rules_are_j0856() {
        let req = request();
        let views = req.views();
        let r = views.request();
        let cases: [&[(u64, &[u32])]; 5] = [
            &[(100, &[0, 1])],
            &[(100, &[0, 1, 2]), (200, &[3])],
            &[(100, &[0, 1]), (100, &[3])],
            &[(100, &[0, 1]), (200, &[3]), (200, &[3])],
            &[(100, &[0]), (100, &[1, 0]), (200, &[3])],
        ];
        for batches in cases {
            let mut plan = Plan::new();
            for (period, points) in batches {
                plan.push(0, BatchKind::Poll, *period, points, 1, 0, &[]);
            }
            let err = validate(&r, &plan.view()).unwrap_err();
            assert_eq!(err.code, J0856, "{batches:?}");
        }
    }
}
