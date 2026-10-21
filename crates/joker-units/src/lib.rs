#![no_std]
#![doc = include_str!("../README.md")]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::panic))]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod catalog;
mod dimension;
mod duration;
mod error;
mod kind;
mod number;
mod quantity;
mod unit;

pub use dimension::{BaseDimension, Dimension};
pub use duration::Duration;
pub use error::{Error, ErrorKind, Span};
pub use kind::{QuantityKind, UnknownQuantityKind};
pub use quantity::{Literal, Quantity};
pub use unit::{Conversion, MAX_SYMBOL_LEN, Unit};
