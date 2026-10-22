use joker_units::{Conversion, Duration, Span};

use crate::ast::{BinaryOp, Node, NodeId, UnaryOp};
use crate::check::{Checked, Const, Detail, Func};
use crate::diag::{Diagnostic, ErrorKind};
use crate::types::{SignalInfo, SignalType, Ty};
use crate::value::Value;

/// The most instructions in a program.
pub const MAX_CODE: usize = 512;
/// The most constants in a program.
pub const MAX_CONSTS: usize = 128;
/// The most distinct signals a program reads.
pub const MAX_INPUTS: usize = 128;
/// The most unit conversions in a program.
pub const MAX_CONVERSIONS: usize = 128;
/// The most stateful function calls in a program.
pub const MAX_STATEFUL: usize = 32;
/// The deepest VM stack a program may need.
pub const MAX_STACK: usize = 128;

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
}

/// The four window functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// Mean of the samples that are not `Bad`.
    Avg,
    /// Minimum.
    Min,
    /// Maximum.
    Max,
    /// Change per second.
    Rate,
}

/// One instruction of the stack VM. Programs are straight-line: there are
/// no jumps, so every instruction runs once per evaluation (ADR 0034).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Push `consts[i]` with `Good` quality.
    Const(u16),
    /// Push the sample of `inputs[i]`.
    Load(u16),
    /// Push the caller's last proposal for `inputs[i]` (`own`).
    Own(u16),
    /// Push whether `inputs[i]` is `Good`.
    IsGood(u16),
    /// Push the quality code of `inputs[i]`.
    QualityOf(u16),
    /// Push the age of `inputs[i]` in seconds.
    Age(u16),
    /// Apply `conversions[i]` to the top of the stack.
    Convert(u16),
    /// Negate.
    Neg,
    /// Logical not.
    Not,
    /// Add.
    Add,
    /// Subtract.
    Sub,
    /// Multiply.
    Mul,
    /// Divide.
    Div,
    /// Compare.
    Cmp(CmpOp),
    /// Logical and, with the symmetric short-circuit of docs/16 §3.2.
    And,
    /// Logical or, with the symmetric short-circuit of docs/16 §3.2.
    Or,
    /// `if c then a else b`: pops `b`, `a`, `c`.
    Select,
    /// Absolute value.
    Abs,
    /// Square root.
    Sqrt,
    /// Power.
    Pow,
    /// Minimum of the top `n` values.
    Min(u8),
    /// Maximum of the top `n` values.
    Max(u8),
    /// `clamp(x, lo, hi)`.
    Clamp,
    /// A window function over `stateful[i]`.
    Window(WindowKind, u16),
    /// `held` over `stateful[i]`.
    Held(u16),
    /// `rose` over `stateful[i]`.
    Rose(u16),
    /// `fell` over `stateful[i]`.
    Fell(u16),
    /// `hyst` over `stateful[i]`.
    Hyst(u16),
    /// Bit `n` of the top value.
    Bit(u8),
    /// `any` of the top `n` values.
    Any(u8),
    /// `all` of the top `n` values.
    All(u8),
    /// Number of true values among the top `n`.
    Count(u8),
    /// Sum of the top `n` values.
    Sum(u8),
    /// Maximum of the top `n` values (selector aggregation).
    Maxs(u8),
    /// Minimum of the top `n` values (selector aggregation).
    Mins(u8),
}

/// A signal read by a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    /// The `SignalId`.
    pub signal: u32,
    /// Its type, to decode the slot.
    pub ty: SignalType,
}

/// The state a stateful call keeps between evaluations, as a range of
/// words in the caller's state arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stateful {
    /// First word in the arena.
    pub offset: u32,
    /// Ring capacity of a window, in samples (0 for the other functions).
    pub capacity: u32,
    /// The hold time of `held`.
    pub duration: Duration,
}

/// Options for [`compile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileOptions {
    /// The period of the group that evaluates the expression; windows hold
    /// `ceil(window / period)` samples (ADR 0034).
    pub period: Duration,
}

/// A compiled expression: straight-line bytecode with fixed-capacity
/// tables. It is self-contained, and evaluating it never
/// allocates.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub(crate) code: [Op; MAX_CODE],
    pub(crate) code_len: u16,
    pub(crate) consts: [Value; MAX_CONSTS],
    pub(crate) consts_len: u16,
    pub(crate) inputs: [Input; MAX_INPUTS],
    pub(crate) inputs_len: u16,
    pub(crate) conversions: [Conversion; MAX_CONVERSIONS],
    pub(crate) conversions_len: u16,
    pub(crate) stateful: [Stateful; MAX_STATEFUL],
    pub(crate) stateful_len: u16,
    pub(crate) state_words: usize,
    pub(crate) max_stack: u16,
    pub(crate) result: Ty,
}

impl Program {
    /// The instructions.
    pub fn code(&self) -> &[Op] {
        self.code.get(..self.code_len as usize).unwrap_or(&[])
    }

    /// The signals the program reads, in first-use order.
    pub fn inputs(&self) -> &[Input] {
        self.inputs.get(..self.inputs_len as usize).unwrap_or(&[])
    }

    /// The constants.
    pub fn consts(&self) -> &[Value] {
        self.consts.get(..self.consts_len as usize).unwrap_or(&[])
    }

    /// The size of the state arena the caller must provide, in `u64` words.
    /// A zeroed arena is the initial state.
    pub fn state_words(&self) -> usize {
        self.state_words
    }

    /// The deepest stack an evaluation reaches.
    pub fn max_stack(&self) -> usize {
        self.max_stack as usize
    }

    /// The type of the result.
    pub fn result_type(&self) -> Ty {
        self.result
    }

    pub(crate) fn stateful(&self, i: u16) -> Option<Stateful> {
        self.stateful
            .get(..self.stateful_len as usize)?
            .get(i as usize)
            .copied()
    }

    pub(crate) fn conversion(&self, i: u16) -> Option<Conversion> {
        self.conversions
            .get(..self.conversions_len as usize)?
            .get(i as usize)
            .copied()
    }

    pub(crate) fn input(&self, i: u16) -> Option<Input> {
        self.inputs().get(i as usize).copied()
    }

    pub(crate) fn constant(&self, i: u16) -> Option<Value> {
        self.consts().get(i as usize).copied()
    }
}

/// Words of a window's header in the state arena.
pub(crate) const WINDOW_HEADER: usize = 10;
/// Words per window sample (value, quality, time).
pub(crate) const WINDOW_SAMPLE: usize = 3;

/// Compiles a checked expression into a program.
///
/// Fails with `J0149` if a fixed capacity is exceeded, and with `J0152` if
/// the period is zero while the expression has a window.
pub fn compile(checked: &Checked, options: CompileOptions) -> Result<Program, Diagnostic> {
    let mut c = Compiler {
        checked,
        options,
        program: Program {
            code: [Op::Neg; MAX_CODE],
            code_len: 0,
            consts: [Value::Bool(false); MAX_CONSTS],
            consts_len: 0,
            inputs: [Input {
                signal: 0,
                ty: SignalType::Bool,
            }; MAX_INPUTS],
            inputs_len: 0,
            conversions: [Conversion::IDENTITY; MAX_CONVERSIONS],
            conversions_len: 0,
            stateful: [Stateful {
                offset: 0,
                capacity: 0,
                duration: Duration::ZERO,
            }; MAX_STATEFUL],
            stateful_len: 0,
            state_words: 0,
            max_stack: 0,
            result: checked.result_type(),
        },
        depth: 0,
    };
    let root = checked.ast().root();
    c.emit(root)?;
    Ok(c.program)
}

struct Compiler<'a> {
    checked: &'a Checked,
    options: CompileOptions,
    program: Program,
    depth: u16,
}

fn too_complex(span: Span) -> Diagnostic {
    Diagnostic::new(ErrorKind::TooComplex, span)
}

fn push_into<T: Copy>(
    table: &mut [T],
    len: &mut u16,
    item: T,
    span: Span,
) -> Result<u16, Diagnostic> {
    let i = *len;
    *table.get_mut(i as usize).ok_or(too_complex(span))? = item;
    *len = i.checked_add(1).ok_or(too_complex(span))?;
    Ok(i)
}

impl Compiler<'_> {
    fn op(&mut self, op: Op, pops: u16, pushes: u16, span: Span) -> Result<(), Diagnostic> {
        let p = &mut self.program;
        push_into(&mut p.code, &mut p.code_len, op, span)?;
        self.depth = self.depth.saturating_sub(pops) + pushes;
        if self.depth as usize > MAX_STACK {
            return Err(too_complex(span));
        }
        self.program.max_stack = self.program.max_stack.max(self.depth);
        Ok(())
    }

    fn constant(&mut self, value: Value, span: Span) -> Result<(), Diagnostic> {
        let p = &mut self.program;
        let existing = p
            .consts()
            .iter()
            .position(|c| c.to_raw() == value.to_raw() && same_variant(c, &value));
        let i = match existing.and_then(|i| u16::try_from(i).ok()) {
            Some(i) => i,
            None => push_into(&mut p.consts, &mut p.consts_len, value, span)?,
        };
        self.op(Op::Const(i), 0, 1, span)
    }

    fn input(&mut self, signal: &SignalInfo, span: Span) -> Result<u16, Diagnostic> {
        let p = &mut self.program;
        let input = Input {
            signal: signal.id,
            ty: signal.ty,
        };
        match p.inputs().iter().position(|i| *i == input) {
            Some(i) => u16::try_from(i).map_err(|_| too_complex(span)),
            None => push_into(&mut p.inputs, &mut p.inputs_len, input, span),
        }
    }

    fn convert(&mut self, conv: Conversion, span: Span) -> Result<(), Diagnostic> {
        if conv.is_identity() {
            return Ok(());
        }
        let p = &mut self.program;
        let i = push_into(&mut p.conversions, &mut p.conversions_len, conv, span)?;
        self.op(Op::Convert(i), 1, 1, span)
    }

    fn stateful(
        &mut self,
        words: usize,
        capacity: u32,
        duration: Duration,
        span: Span,
    ) -> Result<u16, Diagnostic> {
        let offset = u32::try_from(self.program.state_words).map_err(|_| too_complex(span))?;
        self.program.state_words = self
            .program
            .state_words
            .checked_add(words)
            .ok_or(too_complex(span))?;
        let p = &mut self.program;
        push_into(
            &mut p.stateful,
            &mut p.stateful_len,
            Stateful {
                offset,
                capacity,
                duration,
            },
            span,
        )
    }

    fn emit(&mut self, id: NodeId) -> Result<(), Diagnostic> {
        let info = self.checked.info(id);
        let span = self.checked.ast().span(id);
        if let Detail::Const(c) = info.detail {
            let value = match c {
                Const::Bool(b) => Value::Bool(b),
                Const::Float(v) => Value::F64(info.coerce.apply(v)),
                Const::Int(i) if info.coerce.is_identity() => Value::I64(i),
                Const::Int(i) => Value::F64(info.coerce.apply(i as f64)),
                Const::Enum(v) => Value::U64(v),
            };
            return self.constant(value, span);
        }
        self.emit_inner(id, span)?;
        self.convert(info.coerce, span)
    }

    fn emit_inner(&mut self, id: NodeId, span: Span) -> Result<(), Diagnostic> {
        let ast = self.checked.ast();
        match ast.node(id) {
            Node::Literal { .. } | Node::Bool(_) | Node::EnumVariant => Err(too_complex(span)),
            Node::Path | Node::Selector => match self.checked.info(id).detail {
                Detail::Signal(signal) => {
                    let i = self.input(&signal, span)?;
                    self.op(Op::Load(i), 0, 1, span)
                }
                _ => Err(too_complex(span)),
            },
            Node::Unary { op, operand } => {
                self.emit(operand)?;
                let op = match op {
                    UnaryOp::Neg => Op::Neg,
                    UnaryOp::Not => Op::Not,
                };
                self.op(op, 1, 1, span)
            }
            Node::Binary { op, lhs, rhs } => {
                self.emit(lhs)?;
                self.emit(rhs)?;
                let op = match op {
                    BinaryOp::Or => Op::Or,
                    BinaryOp::And => Op::And,
                    BinaryOp::Eq => Op::Cmp(CmpOp::Eq),
                    BinaryOp::Ne => Op::Cmp(CmpOp::Ne),
                    BinaryOp::Lt => Op::Cmp(CmpOp::Lt),
                    BinaryOp::Le => Op::Cmp(CmpOp::Le),
                    BinaryOp::Gt => Op::Cmp(CmpOp::Gt),
                    BinaryOp::Ge => Op::Cmp(CmpOp::Ge),
                    BinaryOp::Add => Op::Add,
                    BinaryOp::Sub => Op::Sub,
                    BinaryOp::Mul => Op::Mul,
                    BinaryOp::Div => Op::Div,
                };
                self.op(op, 2, 1, span)
            }
            Node::If {
                cond,
                then,
                otherwise,
            } => {
                self.emit(cond)?;
                self.emit(then)?;
                self.emit(otherwise)?;
                self.op(Op::Select, 3, 1, span)
            }
            Node::Call { first, count, .. } => {
                let Detail::Call(func) = self.checked.info(id).detail else {
                    return Err(too_complex(span));
                };
                self.call(func, first, count, span)
            }
        }
    }

    fn arg_signal(&mut self, arg: NodeId, span: Span) -> Result<u16, Diagnostic> {
        match self.checked.info(arg).detail {
            Detail::Signal(signal) => self.input(&signal, span),
            _ => Err(too_complex(span)),
        }
    }

    fn arg_duration(&self, arg: NodeId, span: Span) -> Result<Duration, Diagnostic> {
        match self.checked.info(arg).detail {
            Detail::Duration(d) => Ok(d),
            _ => Err(too_complex(span)),
        }
    }

    fn call(&mut self, func: Func, first: u16, count: u16, span: Span) -> Result<(), Diagnostic> {
        let mut args = [NodeId(0); crate::limits::MAX_CALL_ARGS];
        for (slot, a) in args.iter_mut().zip(self.checked.ast().args(first, count)) {
            *slot = *a;
        }
        let n = count;
        let arg = |i: usize| args.get(i).copied().unwrap_or(NodeId(0));
        match func {
            Func::Good | Func::Quality | Func::Age | Func::Own => {
                let i = self.arg_signal(arg(0), span)?;
                let op = match func {
                    Func::Good => Op::IsGood(i),
                    Func::Quality => Op::QualityOf(i),
                    Func::Age => Op::Age(i),
                    _ => Op::Own(i),
                };
                self.op(op, 0, 1, span)
            }
            Func::Avg | Func::Minw | Func::Maxw | Func::Rate => {
                let window = self.arg_duration(arg(1), span)?;
                let period = self.options.period.as_nanos();
                if period == 0 {
                    return Err(Diagnostic::new(ErrorKind::ZeroDuration, span));
                }
                let capacity = window.as_nanos().div_ceil(period).max(1);
                let capacity = u32::try_from(capacity).map_err(|_| too_complex(span))?;
                let deque = matches!(func, Func::Minw | Func::Maxw);
                let words =
                    WINDOW_HEADER + (capacity as usize) * (WINDOW_SAMPLE + usize::from(deque));
                let slot = self.stateful(words, capacity, window, span)?;
                self.emit(arg(0))?;
                let kind = match func {
                    Func::Avg => WindowKind::Avg,
                    Func::Minw => WindowKind::Min,
                    Func::Maxw => WindowKind::Max,
                    _ => WindowKind::Rate,
                };
                self.op(Op::Window(kind, slot), 1, 1, span)
            }
            Func::Held => {
                let d = self.arg_duration(arg(1), span)?;
                let slot = self.stateful(2, 0, d, span)?;
                self.emit(arg(0))?;
                self.op(Op::Held(slot), 1, 1, span)
            }
            Func::Rose | Func::Fell => {
                let slot = self.stateful(1, 0, Duration::ZERO, span)?;
                self.emit(arg(0))?;
                let op = if func == Func::Rose {
                    Op::Rose(slot)
                } else {
                    Op::Fell(slot)
                };
                self.op(op, 1, 1, span)
            }
            Func::Hyst => {
                let slot = self.stateful(1, 0, Duration::ZERO, span)?;
                for i in 0..3 {
                    self.emit(arg(i))?;
                }
                self.op(Op::Hyst(slot), 3, 1, span)
            }
            Func::Bit => {
                let bit = match self.checked.info(arg(1)).detail {
                    Detail::Const(Const::Int(b)) => {
                        u8::try_from(b).map_err(|_| too_complex(span))?
                    }
                    _ => return Err(too_complex(span)),
                };
                self.emit(arg(0))?;
                self.op(Op::Bit(bit), 1, 1, span)
            }
            Func::Abs | Func::Sqrt => {
                self.emit(arg(0))?;
                let op = if func == Func::Abs { Op::Abs } else { Op::Sqrt };
                self.op(op, 1, 1, span)
            }
            Func::Pow => {
                self.emit(arg(0))?;
                self.emit(arg(1))?;
                self.op(Op::Pow, 2, 1, span)
            }
            Func::Clamp => {
                for i in 0..3 {
                    self.emit(arg(i))?;
                }
                self.op(Op::Clamp, 3, 1, span)
            }
            Func::Min | Func::Max => {
                for i in 0..n as usize {
                    self.emit(arg(i))?;
                }
                let k = u8::try_from(n).map_err(|_| too_complex(span))?;
                let op = if func == Func::Min {
                    Op::Min(k)
                } else {
                    Op::Max(k)
                };
                self.op(op, n, 1, span)
            }
            Func::Any | Func::All | Func::Count | Func::Sum | Func::Maxs | Func::Mins => {
                let mut total = 0u16;
                for i in 0..n as usize {
                    let a = arg(i);
                    match self.checked.info(a).detail {
                        Detail::Selector { first, count } => {
                            let a_span = self.checked.ast().span(a);
                            for element in self.checked.elements(first, count) {
                                let input = self.input(&element.signal, a_span)?;
                                self.op(Op::Load(input), 0, 1, a_span)?;
                                self.convert(element.coerce, a_span)?;
                                total += 1;
                            }
                        }
                        _ => {
                            self.emit(a)?;
                            total += 1;
                        }
                    }
                }
                let k = u8::try_from(total).map_err(|_| too_complex(span))?;
                let op = match func {
                    Func::Any => Op::Any(k),
                    Func::All => Op::All(k),
                    Func::Count => Op::Count(k),
                    Func::Sum => Op::Sum(k),
                    Func::Maxs => Op::Maxs(k),
                    _ => Op::Mins(k),
                };
                self.op(op, total, 1, span)
            }
        }
    }
}

fn same_variant(a: &Value, b: &Value) -> bool {
    core::mem::discriminant(a) == core::mem::discriminant(b)
}
