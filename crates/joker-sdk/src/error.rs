//! Errors (ref/04 §4.7.1): a registry code and a bounded English message.

use core::fmt;

use crate::value::{Quality, QualityCode};

/// The result of the SDK and of app tasks.
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// A registry code (`"J-E0101"`), stored inline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Code([u8; 8]);

impl Code {
    /// A code from a static string of at most 8 ASCII bytes; longer codes are
    /// cut.
    pub const fn from_static(s: &'static str) -> Self {
        let b = s.as_bytes();
        let mut out = [0u8; 8];
        let mut i = 0;
        while i < 8 && i < b.len() {
            out[i] = b[i];
            i += 1;
        }
        Self(out)
    }

    /// Builds a code from any text, cut to 8 bytes at a character boundary.
    pub fn new(s: &str) -> Self {
        let mut out = [0u8; 8];
        let n = floor_boundary(s, 8);
        out[..n].copy_from_slice(&s.as_bytes()[..n]);
        Self(out)
    }

    /// The code as text.
    pub fn as_str(&self) -> &str {
        let n = self.0.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.0[..n]).unwrap_or("")
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn floor_boundary(s: &str, limit: usize) -> usize {
    if s.len() <= limit {
        return s.len();
    }
    let mut i = limit;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

const MESSAGE: usize = 112;

/// An error of the SDK or of the app: what a task returns, and what reaches
/// the host as `cycle-result.error`. It is 121 bytes, without allocation, so
/// that a `Result` stays small on the tick path (ADR 0053).
#[derive(Clone, PartialEq, Eq)]
pub struct Error {
    code: Code,
    message: [u8; MESSAGE],
    len: u8,
}

struct Truncating<'a> {
    buf: &'a mut [u8; MESSAGE],
    len: usize,
}

impl fmt::Write for Truncating<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let n = floor_boundary(s, MESSAGE - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

impl Error {
    /// An error with a code and a message, truncated to 112 bytes (ADR 0053).
    pub fn new(code: Code, message: impl fmt::Display) -> Self {
        let mut buf = [0u8; MESSAGE];
        let mut w = Truncating {
            buf: &mut buf,
            len: 0,
        };
        let _ = fmt::write(&mut w, format_args!("{message}"));
        let len = w.len as u8;
        Self {
            code,
            message: buf,
            len,
        }
    }

    /// An error of the app (`J-E0050`).
    pub fn app(message: impl fmt::Display) -> Self {
        Self::new(Code::from_static("J-E0050"), message)
    }

    /// The code.
    pub fn code(&self) -> Code {
        self.code
    }

    /// The message.
    pub fn message(&self) -> &str {
        core::str::from_utf8(&self.message[..self.len as usize]).unwrap_or("")
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message())
    }
}

/// A read refused because of the sample's quality (docs/16 §3.1): `J-E0101`
/// when it is not `Good`, `J-E0102` when it is `Bad`. An unbound optional
/// port reads as `bad(not_connected)` (ref/04 §3.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QualityError {
    /// The port.
    pub port: &'static str,
    /// The quality of the refused sample.
    pub quality: Quality,
}

impl QualityError {
    /// The registry code of this refusal.
    pub fn code(&self) -> Code {
        match self.quality.code {
            QualityCode::Bad => Code::from_static("J-E0102"),
            _ => Code::from_static("J-E0101"),
        }
    }
}

impl fmt::Display for QualityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "port `{}` is {}", self.port, self.quality)
    }
}

impl From<QualityError> for Error {
    fn from(e: QualityError) -> Self {
        Error::new(e.code(), e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::QualityReason;

    #[test]
    fn messages_are_truncated_at_a_character_boundary() {
        let long = "é".repeat(200);
        let e = Error::app(&long);
        assert_eq!(e.message().len(), 112);
        assert!(core::mem::size_of::<Error>() <= 128);
        assert_eq!(e.code().as_str(), "J-E0050");
        assert_eq!(Code::new("J-E0101-extra").as_str(), "J-E0101-");
    }

    #[test]
    fn quality_errors_map_to_their_codes() {
        let bad = QualityError {
            port: "t",
            quality: Quality::bad(QualityReason::CommFailure),
        };
        let e = Error::from(bad);
        assert_eq!(e.code().as_str(), "J-E0102");
        assert_eq!(e.message(), "port `t` is bad(comm_failure)");
        let uncertain = QualityError {
            port: "t",
            quality: Quality::uncertain(QualityReason::Initial),
        };
        assert_eq!(uncertain.code().as_str(), "J-E0101");
    }
}
