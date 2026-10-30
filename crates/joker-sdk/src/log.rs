//! `joker::log::{trace, debug, info, warn, error}!` (ref/04 §4.11).
//!
//! The message and the values of its fields are formatted into one 1 KiB
//! buffer on the stack, truncated when they do not fit, and sent with the
//! host's `log.log`; nothing is allocated, so a cycle task can log
//! (ref/04 §4.13). `enabled` is the host's `log.enabled`: the macros ask it
//! first, so a record the host would drop is never formatted.
//!
//! ```ignore
//! log::warn!("forecast OOD; using the conservative plan");
//! log::info!("level {:.2} m", level; app_mode = "auto", pump = "p1");
//! ```

use core::fmt::{self, Write};

use joker_sdk_sys::joker::runtime::log as host;

/// The size of the buffer a record is formatted into.
pub const BUFFER: usize = 1024;

/// The most fields one record carries; later ones are dropped.
pub const MAX_FIELDS: usize = 8;

/// The level of a log record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Detail for tracing one path.
    Trace,
    /// Detail for debugging.
    Debug,
    /// What the app is doing.
    Info,
    /// Something unexpected the app handles.
    Warn,
    /// Something the app could not do.
    Error,
}

impl Level {
    fn host(self) -> host::Level {
        match self {
            Self::Trace => host::Level::Trace,
            Self::Debug => host::Level::Debug,
            Self::Info => host::Level::Info,
            Self::Warn => host::Level::Warn,
            Self::Error => host::Level::Error,
        }
    }
}

/// Whether the host keeps records of `level` (`log.enabled`). Outside a
/// Wasm component nothing is kept.
pub fn enabled(level: Level) -> bool {
    cfg!(target_family = "wasm") && host::enabled(level.host())
}

/// A fixed buffer that keeps what fits and drops the rest.
#[derive(Debug)]
pub struct Buffer {
    bytes: [u8; BUFFER],
    len: usize,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            bytes: [0; BUFFER],
            len: 0,
        }
    }
}

impl Buffer {
    /// What was written, cut at a character boundary.
    pub fn as_str(&self) -> &str {
        self.slice(0, self.len)
    }

    /// The length written so far.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when nothing was written.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn slice(&self, from: usize, to: usize) -> &str {
        let part = &self.bytes[from.min(self.len)..to.min(self.len)];
        match core::str::from_utf8(part) {
            Ok(s) => s,
            Err(e) => core::str::from_utf8(&part[..e.valid_up_to()]).unwrap_or(""),
        }
    }
}

impl Write for Buffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = BUFFER - self.len;
        let take = s.len().min(room);
        self.bytes[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

/// Where each part of a record lies in its [`Buffer`]: the message, then
/// up to [`MAX_FIELDS`] fields as (name, value range).
#[derive(Debug)]
pub struct Layout {
    /// The end of the message.
    pub message_end: usize,
    /// The fields, as (name, (from, to)).
    pub fields: [(&'static str, (usize, usize)); MAX_FIELDS],
    /// How many fields are set.
    pub len: usize,
}

/// Formats `message` and the values of `fields` into `buf`.
pub fn format(
    buf: &mut Buffer,
    message: fmt::Arguments<'_>,
    fields: &[(&'static str, &dyn fmt::Display)],
) -> Layout {
    let _ = buf.write_fmt(message);
    let message_end = buf.len;
    let mut layout = Layout {
        message_end,
        fields: [("", (0, 0)); MAX_FIELDS],
        len: 0,
    };
    for (name, value) in fields.iter().take(MAX_FIELDS) {
        let from = buf.len;
        let _ = write!(buf, "{value}");
        layout.fields[layout.len] = (name, (from, buf.len));
        layout.len += 1;
    }
    layout
}

/// What the macros call: formats the record on the stack and hands it to
/// the host.
#[doc(hidden)]
pub fn __emit(
    level: Level,
    message: fmt::Arguments<'_>,
    fields: &[(&'static str, &dyn fmt::Display)],
) {
    let mut buf = Buffer::default();
    let layout = format(&mut buf, message, fields);
    let msg = buf.slice(0, layout.message_end);
    let mut pairs: [(&str, &str); MAX_FIELDS] = [("", ""); MAX_FIELDS];
    for (pair, (name, (from, to))) in pairs.iter_mut().zip(layout.fields.iter()).take(layout.len) {
        *pair = (name, buf.slice(*from, *to));
    }
    if cfg!(target_family = "wasm") {
        joker_sdk_sys::log_record(level.host(), msg, &pairs[..layout.len]);
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __joker_log {
    ($level:expr, $fmt:literal $(, $arg:expr)* $(,)? $(; $($name:ident = $value:expr),+ $(,)?)?) => {{
        let level = $level;
        if $crate::log::enabled(level) {
            $crate::log::__emit(
                level,
                ::core::format_args!($fmt $(, $arg)*),
                &[$($((::core::stringify!($name), &$value as &dyn ::core::fmt::Display)),+)?],
            );
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __joker_trace {
    ($($t:tt)*) => { $crate::__joker_log!($crate::log::Level::Trace, $($t)*) };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __joker_debug {
    ($($t:tt)*) => { $crate::__joker_log!($crate::log::Level::Debug, $($t)*) };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __joker_info {
    ($($t:tt)*) => { $crate::__joker_log!($crate::log::Level::Info, $($t)*) };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __joker_warn {
    ($($t:tt)*) => { $crate::__joker_log!($crate::log::Level::Warn, $($t)*) };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __joker_error {
    ($($t:tt)*) => { $crate::__joker_log!($crate::log::Level::Error, $($t)*) };
}

/// A `debug` record: `debug!("fmt", args…; name = value, …)`.
pub use crate::__joker_debug as debug;
/// An `error` record.
pub use crate::__joker_error as error;
/// An `info` record.
pub use crate::__joker_info as info;
/// A `trace` record.
pub use crate::__joker_trace as trace;
/// A `warn` record.
pub use crate::__joker_warn as warn;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_formatted_on_the_stack_with_its_fields() {
        let mut buf = Buffer::default();
        let level = 1.5;
        let layout = format(
            &mut buf,
            format_args!("level {level:.2} m"),
            &[("app_mode", &"auto"), ("pump", &"p1")],
        );
        assert_eq!(buf.slice(0, layout.message_end), "level 1.50 m");
        assert_eq!(layout.len, 2);
        assert_eq!(layout.fields[0].0, "app_mode");
        let (from, to) = layout.fields[0].1;
        assert_eq!(buf.slice(from, to), "auto");
        let (from, to) = layout.fields[1].1;
        assert_eq!(buf.slice(from, to), "p1");
    }

    #[test]
    fn a_record_longer_than_the_buffer_is_cut_at_a_character() {
        let mut buf = Buffer::default();
        let long = "é".repeat(BUFFER);
        let layout = format(&mut buf, format_args!("{long}"), &[]);
        let msg = buf.slice(0, layout.message_end);
        assert!(msg.len() <= BUFFER);
        assert!(msg.chars().all(|c| c == 'é'));
    }

    #[test]
    fn nothing_is_kept_outside_a_component() {
        assert!(!enabled(Level::Error));
        crate::log::warn!("not kept"; reason = "host build");
    }
}
