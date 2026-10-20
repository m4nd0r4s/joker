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
mod program;
mod quality;
mod types;
mod value;
mod vm;

pub use check::{CheckOptions, Checked, Const, Detail, Element, Func, NodeInfo, check};
pub use diag::{Diagnostic, ErrorKind, Expected};
pub use parser::parse;
pub use program::{
    CmpOp, CompileOptions, Input, MAX_CODE, MAX_CONSTS, MAX_CONVERSIONS, MAX_INPUTS, MAX_STACK,
    MAX_STATEFUL, Op, Program, Stateful, WindowKind, compile,
};
pub use quality::{Quality, QualityCode, Reason};
pub use types::{EnumId, Env, Repr, SignalInfo, SignalType, Ty};
pub use value::{Output, QualityError, Value};
pub use vm::{EvalError, Inputs, Reading, eval};
