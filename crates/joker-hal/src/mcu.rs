//! The traits of the Joker Micro drivers that `embedded-hal` 1.0 does not
//! cover (ref/06 §3.7): the `chip-*` modules of `joker-micro` implement
//! them over the HAL of each family, and the drivers use only them.
//!
//! `CanFrames` is reserved for 1.1 with CAN (ADR 0099).

use core::future::Future;

/// One ADC channel (ref/06 §3.7): `embedded-hal` 1.0 has no ADC trait.
pub trait AdcRead {
    /// The error of a conversion.
    type Error: core::fmt::Debug;

    /// One conversion: the raw code, in `0..=(1 << bits) - 1`. Scaling is
    /// the core's (ref/03 §3).
    fn read(&mut self) -> impl Future<Output = Result<u16, Self::Error>>;

    /// The resolution in bits, as the board declares it (`[adc] bits`,
    /// ref/03 §2.3.3).
    fn bits(&self) -> u8;
}

#[cfg(test)]
mod tests {
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};

    use super::AdcRead;

    struct Fixed {
        code: u16,
        bits: u8,
    }

    impl AdcRead for Fixed {
        type Error = ();

        async fn read(&mut self) -> Result<u16, ()> {
            if self.code < (1 << self.bits) {
                Ok(self.code)
            } else {
                Err(())
            }
        }

        fn bits(&self) -> u8 {
            self.bits
        }
    }

    fn ready<F: core::future::Future>(fut: F) -> F::Output {
        let mut fut = pin!(fut);
        match fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("a fixed channel converts at once"),
        }
    }

    #[test]
    fn a_channel_reads_a_code_within_its_resolution() {
        let mut adc = Fixed {
            code: 4095,
            bits: 12,
        };
        assert_eq!(adc.bits(), 12);
        assert_eq!(ready(adc.read()), Ok(4095));
        let mut over = Fixed {
            code: 4096,
            bits: 12,
        };
        assert_eq!(ready(over.read()), Err(()));
    }
}
