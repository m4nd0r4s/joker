//! Bounded types, without allocation (ref/04 §4.7): `BoundedVec`,
//! `BoundedString`, `BoundedBytes` and `Bits`.

use core::fmt;
use core::ops::Deref;

/// A vector of at most `N` items, stored inline.
#[derive(Clone, Copy)]
pub struct BoundedVec<T, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> Default for BoundedVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> BoundedVec<T, N> {
    /// An empty vector.
    pub fn new() -> Self {
        Self {
            items: [T::default(); N],
            len: 0,
        }
    }

    /// Appends `v`; when full, gives it back.
    pub fn push(&mut self, v: T) -> Result<(), T> {
        match self.items.get_mut(self.len) {
            Some(slot) => {
                *slot = v;
                self.len += 1;
                Ok(())
            }
            None => Err(v),
        }
    }
}

impl<T, const N: usize> BoundedVec<T, N> {
    /// The items.
    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }
}

impl<T, const N: usize> Deref for BoundedVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: PartialEq, const N: usize> PartialEq for BoundedVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for BoundedVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

/// UTF-8 text of at most `N` bytes, stored inline.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedString<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Default for BoundedString<N> {
    fn default() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> BoundedString<N> {
    /// The text.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }

    /// The text cut to `N` bytes at a character boundary.
    pub fn truncated(s: &str) -> Self {
        let mut n = s.len().min(N);
        while n > 0 && !s.is_char_boundary(n) {
            n -= 1;
        }
        let mut out = Self::default();
        out.bytes[..n].copy_from_slice(&s.as_bytes()[..n]);
        out.len = n;
        out
    }
}

impl<const N: usize> TryFrom<&str> for BoundedString<N> {
    type Error = usize;

    /// The text, or its length when it is longer than `N` bytes.
    fn try_from(s: &str) -> Result<Self, usize> {
        if s.len() > N {
            return Err(s.len());
        }
        Ok(Self::truncated(s))
    }
}

impl<const N: usize> Deref for BoundedString<N> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl<const N: usize> fmt::Debug for BoundedString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<const N: usize> fmt::Display for BoundedString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bytes, at most `N`, stored inline.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedBytes<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Default for BoundedBytes<N> {
    fn default() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> BoundedBytes<N> {
    /// The bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl<const N: usize> TryFrom<&[u8]> for BoundedBytes<N> {
    type Error = usize;

    /// The bytes, or their length when there are more than `N`.
    fn try_from(b: &[u8]) -> Result<Self, usize> {
        if b.len() > N {
            return Err(b.len());
        }
        let mut out = Self::default();
        out.bytes[..b.len()].copy_from_slice(b);
        out.len = b.len();
        Ok(out)
    }
}

impl<const N: usize> Deref for BoundedBytes<N> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const N: usize> fmt::Debug for BoundedBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}

/// A word of `N` bits (1–64), `bits<N>` of ref/00 §6.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bits<const N: usize>(u64);

impl<const N: usize> Bits<N> {
    const MASK: u64 = if N >= 64 { u64::MAX } else { (1u64 << N) - 1 };

    /// The word, without the bits above `N`.
    pub const fn new(bits: u64) -> Self {
        Self(bits & Self::MASK)
    }

    /// Bit `n`.
    pub fn bit(&self, n: usize) -> bool {
        n < N && self.0 & (1u64 << n) != 0
    }

    /// The word.
    pub fn bits(&self) -> u64 {
        self.0
    }
}

impl<const N: usize> fmt::Debug for Bits<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bits<{N}>({:#b})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_types_keep_their_bound() {
        let mut v: BoundedVec<u8, 2> = BoundedVec::new();
        assert_eq!(v.push(1), Ok(()));
        assert_eq!(v.push(2), Ok(()));
        assert_eq!(v.push(3), Err(3));
        assert_eq!(v.iter().copied().sum::<u8>(), 3);
        assert_eq!(BoundedString::<3>::try_from("abcd"), Err(4));
        assert_eq!(BoundedString::<3>::truncated("aéb").as_str(), "aé");
        assert_eq!(
            BoundedBytes::<2>::try_from(&[1u8, 2][..]).map(|b| b.len()),
            Ok(2)
        );
        let b = Bits::<4>::new(0b1_0101);
        assert_eq!(b.bits(), 0b0101);
        assert!(b.bit(2) && !b.bit(1) && !b.bit(4));
    }
}
