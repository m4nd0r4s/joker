#![no_std]
#![doc = include_str!("../README.md")]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::panic))]
#![allow(clippy::result_large_err)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod ast;
mod check;
mod diag;
pub mod lexer;
pub mod limits;
mod parser;
mod types;

pub use check::{CheckOptions, Checked, Const, Detail, Element, Func, NodeInfo, check};
pub use diag::{Diagnostic, ErrorKind, Expected};
pub use parser::parse;
pub use types::{EnumId, Env, Repr, SignalInfo, SignalType, Ty};
