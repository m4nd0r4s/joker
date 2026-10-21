use joker_units::{Conversion, Duration, Literal, Span, Unit};

use crate::ast::{Ast, BinaryOp, Node, NodeId, UnaryOp};
use crate::diag::{Diagnostic, ErrorKind, Expected};
use crate::limits::{MAX_ELEMENTS, MAX_NODES};
use crate::parser;
use crate::types::{Env, Repr, SignalInfo, Ty};

/// The functions of docs/02 §10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Func {
    /// `abs(x)`.
    Abs,
    /// `min(a, b, …)`.
    Min,
    /// `max(a, b, …)`.
    Max,
    /// `clamp(x, lo, hi)`.
    Clamp,
    /// `sqrt(x)`.
    Sqrt,
    /// `pow(x, n)`.
    Pow,
    /// `good(x)`: the quality of signal `x` is `Good`.
    Good,
    /// `quality(x)`: the quality code of signal `x` (0 good, 1 uncertain, 2 bad).
    Quality,
    /// `age(x)`: seconds since signal `x` was acquired.
    Age,
    /// `avg(x, window)`.
    Avg,
    /// `minw(x, window)`.
    Minw,
    /// `maxw(x, window)`.
    Maxw,
    /// `rate(x, window)`: change per second over the window.
    Rate,
    /// `held(cond, duration)`.
    Held,
    /// `rose(cond)`.
    Rose,
    /// `fell(cond)`.
    Fell,
    /// `hyst(x, lo, hi)`.
    Hyst,
    /// `bit(x, n)`.
    Bit,
    /// `own(x)`: the caller's last proposal for output `x` (class methods only).
    Own,
    /// `any(…)`.
    Any,
    /// `all(…)`.
    All,
    /// `count(…)`: how many elements are true.
    Count,
    /// `sum(…)`.
    Sum,
    /// `maxs(…)`.
    Maxs,
    /// `mins(…)`.
    Mins,
}

const FUNCS: [(&str, Func); 25] = [
    ("abs", Func::Abs),
    ("min", Func::Min),
    ("max", Func::Max),
    ("clamp", Func::Clamp),
    ("sqrt", Func::Sqrt),
    ("pow", Func::Pow),
    ("good", Func::Good),
    ("quality", Func::Quality),
    ("age", Func::Age),
    ("avg", Func::Avg),
    ("minw", Func::Minw),
    ("maxw", Func::Maxw),
    ("rate", Func::Rate),
    ("held", Func::Held),
    ("rose", Func::Rose),
    ("fell", Func::Fell),
    ("hyst", Func::Hyst),
    ("bit", Func::Bit),
    ("own", Func::Own),
    ("any", Func::Any),
    ("all", Func::All),
    ("count", Func::Count),
    ("sum", Func::Sum),
    ("maxs", Func::Maxs),
    ("mins", Func::Mins),
];

impl Func {
    /// Looks a function up by name.
    pub fn from_name(name: &str) -> Option<Func> {
        FUNCS.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
    }

    /// The function name.
    pub fn name(self) -> &'static str {
        FUNCS
            .iter()
            .find(|(_, f)| *f == self)
            .map_or("", |(n, _)| n)
    }

    /// True for the aggregations, the only functions that take selectors.
    pub fn is_aggregation(self) -> bool {
        matches!(
            self,
            Func::Any | Func::All | Func::Count | Func::Sum | Func::Maxs | Func::Mins
        )
    }

    /// True for the functions whose result depends on earlier ticks.
    pub fn is_stateful(self) -> bool {
        matches!(
            self,
            Func::Avg
                | Func::Minw
                | Func::Maxw
                | Func::Rate
                | Func::Held
                | Func::Rose
                | Func::Fell
                | Func::Hyst
        )
    }

    fn arity(self) -> (u8, u8) {
        match self {
            Func::Abs
            | Func::Sqrt
            | Func::Good
            | Func::Quality
            | Func::Age
            | Func::Rose
            | Func::Fell
            | Func::Own => (1, 1),
            Func::Pow
            | Func::Avg
            | Func::Minw
            | Func::Maxw
            | Func::Rate
            | Func::Held
            | Func::Bit => (2, 2),
            Func::Clamp | Func::Hyst => (3, 3),
            Func::Min | Func::Max => (2, u8::MAX),
            Func::Any | Func::All | Func::Count | Func::Sum | Func::Maxs | Func::Mins => {
                (1, u8::MAX)
            }
        }
    }
}

/// A constant known at compile time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Const {
    /// A boolean.
    Bool(bool),
    /// A float, in the node's unit.
    Float(f64),
    /// An integer literal without a unit.
    Int(i64),
    /// The numeric value of an enum variant.
    Enum(u64),
}

/// What the checker learned about a node, beyond its type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Detail {
    /// Nothing more.
    None,
    /// A literal.
    Const(Const),
    /// A signal read.
    Signal(SignalInfo),
    /// A call of this function.
    Call(Func),
    /// A duration argument (window or hold time).
    Duration(Duration),
    /// A selector, expanded to `elements[first..first + count]`.
    Selector {
        /// Index of the first element.
        first: u16,
        /// Number of elements.
        count: u16,
    },
}

/// The checker's annotation of one node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeInfo {
    /// The type of the node's value, before [`NodeInfo::coerce`].
    pub ty: Ty,
    /// The conversion the parent applies to this value (unit alignment).
    pub coerce: Conversion,
    /// Literal values, signals, functions and selectors.
    pub detail: Detail,
}

const EMPTY_INFO: NodeInfo = NodeInfo {
    ty: Ty::Bool,
    coerce: Conversion::IDENTITY,
    detail: Detail::None,
};

/// One signal of an expanded selector, with the conversion into the
/// aggregation's unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Element {
    /// The signal.
    pub signal: SignalInfo,
    /// Its conversion into the unit of the aggregation.
    pub coerce: Conversion,
}

/// Where an expression is compiled (ref/01 §3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckOptions {
    /// `own()` is only valid inside class methods (docs/17); elsewhere it is `J0306`.
    pub in_class_method: bool,
}

/// A parsed and checked expression: the syntax tree, the type of every
/// node and the expanded selectors. Fixed capacity, no allocation.
#[derive(Debug, Clone)]
pub struct Checked {
    ast: Ast,
    infos: [NodeInfo; MAX_NODES],
    elements: [Option<Element>; MAX_ELEMENTS],
    elements_len: u16,
}

impl Checked {
    /// The syntax tree.
    pub fn ast(&self) -> &Ast {
        &self.ast
    }

    /// The annotation of a node.
    pub fn info(&self, id: NodeId) -> NodeInfo {
        self.infos.get(id.index()).copied().unwrap_or(EMPTY_INFO)
    }

    /// The elements of a selector node.
    pub fn elements(&self, first: u16, count: u16) -> impl Iterator<Item = Element> + '_ {
        let end = first as usize + count as usize;
        self.elements
            .get(first as usize..end)
            .unwrap_or(&[])
            .iter()
            .flatten()
            .copied()
    }

    /// The type of the whole expression.
    pub fn result_type(&self) -> Ty {
        self.info(self.ast.root()).ty
    }

    /// Fails with `J0304` unless the expression is a `bool`, as permissives,
    /// alarm conditions and `reset_when` require.
    pub fn expect_bool(&self) -> Result<(), Diagnostic> {
        match self.result_type() {
            Ty::Bool => Ok(()),
            found => Err(Diagnostic::new(
                ErrorKind::TypeMismatch {
                    expected: Expected::Bool,
                    found,
                },
                self.ast.span(self.ast.root()),
            )),
        }
    }

    /// Fails with `J0301`/`J0108` unless the expression is a number
    /// convertible to `unit`, as `range` limits require (E6), and returns
    /// the conversion.
    pub fn expect_unit(&self, unit: &Unit) -> Result<Conversion, Diagnostic> {
        let span = self.ast.span(self.ast.root());
        match self.result_type() {
            Ty::Number { unit: found, .. } => conversion(&found, unit, span),
            found => Err(Diagnostic::new(
                ErrorKind::TypeMismatch {
                    expected: Expected::Number,
                    found,
                },
                span,
            )),
        }
    }
}

/// Parses and checks an expression against the registry.
///
/// Unknown signals are `J0302`, unknown enum variants `J0303`, dimension
/// errors `J0301`, quantity kind errors `J0108`, type errors `J0304`,
/// windows above 15 min `J0305`, `own()` outside a method `J0306`, and
/// unknown functions or bad arguments `J0307` (ADR 0034).
pub fn check(src: &str, env: &dyn Env, options: CheckOptions) -> Result<Checked, Diagnostic> {
    let ast = parser::parse(src)?;
    let mut checker = Checker {
        src,
        env,
        options,
        out: Checked {
            ast,
            infos: [EMPTY_INFO; MAX_NODES],
            elements: [None; MAX_ELEMENTS],
            elements_len: 0,
        },
    };
    let root = checker.out.ast.root();
    checker.check(root)?;
    Ok(checker.out)
}

struct Checker<'a> {
    src: &'a str,
    env: &'a dyn Env,
    options: CheckOptions,
    out: Checked,
}

fn conversion(from: &Unit, to: &Unit, span: Span) -> Result<Conversion, Diagnostic> {
    from.conversion_to(to).map_err(|e| {
        let kind = match e.kind {
            joker_units::ErrorKind::IncompatibleKind => ErrorKind::IncompatibleKinds {
                lhs: *to,
                rhs: *from,
            },
            _ => ErrorKind::IncompatibleUnits {
                lhs: *to,
                rhs: *from,
            },
        };
        Diagnostic::new(kind, span)
    })
}

fn unit_error(e: joker_units::Error, span: Span) -> Diagnostic {
    let kind = match e.kind {
        joker_units::ErrorKind::SymbolTooLong | joker_units::ErrorKind::ExponentOverflow => {
            ErrorKind::TooComplex
        }
        other => ErrorKind::Literal(other),
    };
    Diagnostic::new(kind, span)
}

impl Checker<'_> {
    fn text(&self, span: Span) -> &str {
        self.src.get(span.range()).unwrap_or("")
    }

    fn span(&self, id: NodeId) -> Span {
        self.out.ast.span(id)
    }

    fn set(&mut self, id: NodeId, ty: Ty, detail: Detail) -> Ty {
        if let Some(info) = self.out.infos.get_mut(id.index()) {
            info.ty = ty;
            info.detail = detail;
        }
        ty
    }

    fn coerce(&mut self, id: NodeId, conv: Conversion) {
        if let Some(info) = self.out.infos.get_mut(id.index()) {
            info.coerce = conv;
            if let (false, Ty::Number { repr, .. }) = (conv.is_identity(), &mut info.ty) {
                *repr = Repr::Float;
            }
        }
    }

    fn is_const(&self, id: NodeId) -> bool {
        matches!(self.out.info(id).detail, Detail::Const(_))
    }

    fn mismatch(&self, id: NodeId, expected: Expected, found: Ty) -> Diagnostic {
        Diagnostic::new(ErrorKind::TypeMismatch { expected, found }, self.span(id))
    }

    fn check(&mut self, id: NodeId) -> Result<Ty, Diagnostic> {
        let span = self.span(id);
        match self.out.ast.node(id) {
            Node::Literal { unit_start } => self.literal(id, span, unit_start),
            Node::Bool(b) => Ok(self.set(id, Ty::Bool, Detail::Const(Const::Bool(b)))),
            Node::Path => {
                let info = self.signal(span)?;
                let ty = Ty::of_signal(&info)
                    .ok_or(Diagnostic::new(ErrorKind::UnsupportedSignalType, span))?;
                Ok(self.set(id, ty, Detail::Signal(info)))
            }
            Node::Selector => Err(Diagnostic::new(ErrorKind::SelectorNotAllowed, span)),
            Node::EnumVariant => {
                let text = self.text(span);
                let (name, variant) = text.split_once('.').unwrap_or((text, ""));
                let (enum_id, value) = self
                    .env
                    .enum_variant(name, variant)
                    .ok_or(Diagnostic::new(ErrorKind::UnknownEnumVariant, span))?;
                Ok(self.set(id, Ty::Enum(enum_id), Detail::Const(Const::Enum(value))))
            }
            Node::Unary { op, operand } => {
                let ty = self.check(operand)?;
                match (op, ty) {
                    (UnaryOp::Not, Ty::Bool) => Ok(self.set(id, Ty::Bool, Detail::None)),
                    (UnaryOp::Not, found) => Err(self.mismatch(operand, Expected::Bool, found)),
                    (UnaryOp::Neg, Ty::Number { unit, .. }) if unit.is_affine() => {
                        Err(Diagnostic::new(ErrorKind::AffineOperation { unit }, span))
                    }
                    (UnaryOp::Neg, Ty::Number { unit, repr }) => {
                        Ok(self.set(id, Ty::Number { unit, repr }, Detail::None))
                    }
                    (UnaryOp::Neg, found) => Err(self.mismatch(operand, Expected::Number, found)),
                }
            }
            Node::Binary { op, lhs, rhs } => self.binary(id, op, lhs, rhs),
            Node::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.check(cond)?;
                if c != Ty::Bool {
                    return Err(self.mismatch(cond, Expected::Bool, c));
                }
                let a = self.check(then)?;
                let b = self.check(otherwise)?;
                let ty = self.same_type(a, otherwise, b)?;
                Ok(self.set(id, ty, Detail::None))
            }
            Node::Call { name, first, count } => self.call(id, name, first, count),
        }
    }

    fn signal(&self, span: Span) -> Result<SignalInfo, Diagnostic> {
        self.env
            .signal(self.text(span))
            .ok_or(Diagnostic::new(ErrorKind::UnknownSignal, span))
    }

    fn literal(&mut self, id: NodeId, span: Span, unit_start: u32) -> Result<Ty, Diagnostic> {
        let text = self.text(span);
        let literal = Literal::parse(text)
            .map_err(|e| Diagnostic::new(ErrorKind::Literal(e.kind), e.span.shifted(span.start)))?;
        let (ty, value) = match literal {
            Literal::Number(v) => {
                let int = if unit_start == span.end && text.bytes().all(|b| b.is_ascii_digit()) {
                    text.parse::<i64>().ok()
                } else {
                    None
                };
                match int {
                    Some(i) => (
                        Ty::Number {
                            unit: Unit::ONE,
                            repr: Repr::Signed,
                        },
                        Const::Int(i),
                    ),
                    None => (Ty::SCALAR, Const::Float(v)),
                }
            }
            Literal::Quantity(q) => (
                Ty::Number {
                    unit: q.unit,
                    repr: Repr::Float,
                },
                Const::Float(q.value),
            ),
        };
        Ok(self.set(id, ty, Detail::Const(value)))
    }

    fn number(&mut self, id: NodeId) -> Result<(Unit, Repr), Diagnostic> {
        match self.check(id)? {
            Ty::Number { unit, repr } => Ok((unit, repr)),
            found => Err(self.mismatch(id, Expected::Number, found)),
        }
    }

    fn boolean(&mut self, id: NodeId) -> Result<(), Diagnostic> {
        match self.check(id)? {
            Ty::Bool => Ok(()),
            found => Err(self.mismatch(id, Expected::Bool, found)),
        }
    }

    fn align(&mut self, id: NodeId, from: &Unit, to: &Unit) -> Result<(), Diagnostic> {
        let conv = conversion(from, to, self.span(id))?;
        self.coerce(id, conv);
        Ok(())
    }

    fn same_type(&mut self, a: Ty, b_id: NodeId, b: Ty) -> Result<Ty, Diagnostic> {
        match (a, b) {
            (Ty::Bool, Ty::Bool) => Ok(Ty::Bool),
            (Ty::Enum(x), Ty::Enum(y)) if x == y => Ok(Ty::Enum(x)),
            (Ty::Enum(_), Ty::Enum(_)) => {
                Err(Diagnostic::new(ErrorKind::EnumMismatch, self.span(b_id)))
            }
            (Ty::Number { unit: ua, .. }, Ty::Number { unit: ub, .. }) => {
                self.align(b_id, &ub, &ua)?;
                Ok(Ty::Number {
                    unit: ua,
                    repr: Repr::Float,
                })
            }
            (_, found) => Err(self.mismatch(b_id, expected_like(&a), found)),
        }
    }

    fn binary(
        &mut self,
        id: NodeId,
        op: BinaryOp,
        lhs: NodeId,
        rhs: NodeId,
    ) -> Result<Ty, Diagnostic> {
        let span = self.span(id);
        match op {
            BinaryOp::And | BinaryOp::Or => {
                self.boolean(lhs)?;
                self.boolean(rhs)?;
                Ok(self.set(id, Ty::Bool, Detail::None))
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                let a = self.check(lhs)?;
                let b = self.check(rhs)?;
                match (a, b) {
                    (Ty::Number { unit: ua, .. }, Ty::Number { unit: ub, .. }) => {
                        self.compare_numbers(lhs, ua, rhs, ub)?;
                    }
                    _ => {
                        self.same_type(a, rhs, b)?;
                    }
                }
                Ok(self.set(id, Ty::Bool, Detail::None))
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let (ua, _) = self.ordered(lhs)?;
                let (ub, _) = self.ordered(rhs)?;
                self.compare_numbers(lhs, ua, rhs, ub)?;
                Ok(self.set(id, Ty::Bool, Detail::None))
            }
            BinaryOp::Add | BinaryOp::Sub => {
                let (ua, _) = self.number(lhs)?;
                let (ub, _) = self.number(rhs)?;
                let unit = match (ua.is_affine(), ub.is_affine(), op) {
                    (true, true, BinaryOp::Sub) => {
                        self.align(rhs, &ub, &ua)?;
                        ua.difference()
                    }
                    (true, true, _) => {
                        return Err(Diagnostic::new(
                            ErrorKind::AffineOperation { unit: ua },
                            span,
                        ));
                    }
                    (true, false, _) => {
                        self.align(rhs, &ub, &ua.difference())?;
                        ua
                    }
                    (false, true, BinaryOp::Add) => {
                        self.align(lhs, &ua, &ub.difference())?;
                        ub
                    }
                    (false, true, _) => {
                        return Err(Diagnostic::new(
                            ErrorKind::AffineOperation { unit: ub },
                            span,
                        ));
                    }
                    (false, false, _) => {
                        self.align(rhs, &ub, &ua)?;
                        ua.with_kind(ua.kind().or(ub.kind()))
                    }
                };
                Ok(self.set(
                    id,
                    Ty::Number {
                        unit,
                        repr: Repr::Float,
                    },
                    Detail::None,
                ))
            }
            BinaryOp::Mul | BinaryOp::Div => {
                let (ua, _) = self.number(lhs)?;
                let (ub, _) = self.number(rhs)?;
                for (u, side) in [(ua, lhs), (ub, rhs)] {
                    if u.is_affine() {
                        return Err(Diagnostic::new(
                            ErrorKind::AffineOperation { unit: u },
                            self.span(side),
                        ));
                    }
                }
                let unit = if op == BinaryOp::Mul {
                    ua.checked_mul(&ub)
                } else {
                    ua.checked_div(&ub)
                }
                .map_err(|e| unit_error(e, span))?;
                Ok(self.set(
                    id,
                    Ty::Number {
                        unit,
                        repr: Repr::Float,
                    },
                    Detail::None,
                ))
            }
        }
    }

    fn ordered(&mut self, id: NodeId) -> Result<(Unit, Repr), Diagnostic> {
        match self.check(id)? {
            Ty::Number { unit, repr } => Ok((unit, repr)),
            found => Err(self.mismatch(id, Expected::Ordered, found)),
        }
    }

    fn compare_numbers(
        &mut self,
        lhs: NodeId,
        ua: Unit,
        rhs: NodeId,
        ub: Unit,
    ) -> Result<(), Diagnostic> {
        if self.is_const(lhs) && !self.is_const(rhs) {
            self.align(lhs, &ua, &ub)
        } else {
            self.align(rhs, &ub, &ua)
        }
    }

    fn call(&mut self, id: NodeId, name: Span, first: u16, count: u16) -> Result<Ty, Diagnostic> {
        let span = self.span(id);
        let func = Func::from_name(self.text(name))
            .ok_or(Diagnostic::new(ErrorKind::UnknownFunction, name))?;
        let mut args = [NodeId(0); crate::limits::MAX_CALL_ARGS];
        let n = count as usize;
        for (slot, arg) in args.iter_mut().zip(self.out.ast.args(first, count)) {
            *slot = *arg;
        }
        let args = args.get(..n).unwrap_or(&[]);
        let (min, max) = func.arity();
        if n < min as usize || n > max as usize {
            return Err(Diagnostic::new(ErrorKind::WrongArity { min, max }, span));
        }
        let arg = |i: usize| args.get(i).copied().unwrap_or(NodeId(0));
        let float = |unit: Unit| Ty::Number {
            unit,
            repr: Repr::Float,
        };
        let ty = match func {
            Func::Abs => {
                let (u, _) = self.number(arg(0))?;
                self.not_affine(&u, arg(0))?;
                float(u)
            }
            Func::Min | Func::Max => {
                let (u, _) = self.number(arg(0))?;
                for a in args.iter().skip(1) {
                    let (ua, _) = self.number(*a)?;
                    self.align(*a, &ua, &u)?;
                }
                float(u)
            }
            Func::Clamp => {
                let (u, _) = self.number(arg(0))?;
                for a in [arg(1), arg(2)] {
                    let (ua, _) = self.number(a)?;
                    self.align(a, &ua, &u)?;
                }
                float(u)
            }
            Func::Sqrt => {
                let (u, _) = self.number(arg(0))?;
                let root = u.checked_sqrt().map_err(|_| {
                    Diagnostic::new(
                        ErrorKind::FractionalDimension { unit: u },
                        self.span(arg(0)),
                    )
                })?;
                float(root)
            }
            Func::Pow => self.pow(arg(0), arg(1))?,
            Func::Good => {
                self.signal_arg(arg(0))?;
                Ty::Bool
            }
            Func::Quality => {
                self.signal_arg(arg(0))?;
                Ty::Number {
                    unit: Unit::ONE,
                    repr: Repr::Unsigned,
                }
            }
            Func::Age => {
                self.signal_arg(arg(0))?;
                float(Unit::SECOND)
            }
            Func::Avg | Func::Minw | Func::Maxw | Func::Rate => {
                let (u, _) = self.number(arg(0))?;
                self.duration_arg(arg(1), true)?;
                if func == Func::Rate {
                    float(
                        u.difference()
                            .checked_div(&Unit::SECOND)
                            .map_err(|e| unit_error(e, span))?,
                    )
                } else {
                    float(u)
                }
            }
            Func::Held => {
                self.boolean(arg(0))?;
                self.duration_arg(arg(1), false)?;
                Ty::Bool
            }
            Func::Rose | Func::Fell => {
                self.boolean(arg(0))?;
                Ty::Bool
            }
            Func::Hyst => {
                let (u, _) = self.number(arg(0))?;
                for a in [arg(1), arg(2)] {
                    let (ua, _) = self.number(a)?;
                    self.align(a, &ua, &u)?;
                }
                Ty::Bool
            }
            Func::Bit => {
                match self.check(arg(0))? {
                    Ty::Number {
                        repr: Repr::Signed | Repr::Unsigned,
                        ..
                    } => {}
                    found => return Err(self.mismatch(arg(0), Expected::Integer, found)),
                }
                match self.const_int(arg(1))? {
                    Some(0..=63) => {}
                    _ => {
                        return Err(Diagnostic::new(
                            ErrorKind::ExpectedConstant,
                            self.span(arg(1)),
                        ));
                    }
                }
                Ty::Bool
            }
            Func::Own => {
                if !self.options.in_class_method {
                    return Err(Diagnostic::new(ErrorKind::OwnOutsideMethod, span));
                }
                let info = self.signal_arg(arg(0))?;
                Ty::of_signal(&info).ok_or(Diagnostic::new(
                    ErrorKind::UnsupportedSignalType,
                    self.span(arg(0)),
                ))?
            }
            Func::Any | Func::All | Func::Count => {
                for a in args {
                    self.aggregate_arg(*a, AggregateKind::Bool)?;
                }
                if func == Func::Count {
                    Ty::Number {
                        unit: Unit::ONE,
                        repr: Repr::Unsigned,
                    }
                } else {
                    Ty::Bool
                }
            }
            Func::Sum | Func::Maxs | Func::Mins => {
                let mut unit = None;
                for a in args {
                    let u = self.aggregate_arg(*a, AggregateKind::Number(unit))?;
                    unit = unit.or(u);
                }
                let u = unit.unwrap_or(Unit::ONE);
                if func == Func::Sum && u.is_affine() {
                    return Err(Diagnostic::new(
                        ErrorKind::AffineOperation { unit: u },
                        span,
                    ));
                }
                float(u)
            }
        };
        Ok(self.set(id, ty, Detail::Call(func)))
    }

    fn not_affine(&self, unit: &Unit, id: NodeId) -> Result<(), Diagnostic> {
        if unit.is_affine() {
            return Err(Diagnostic::new(
                ErrorKind::AffineOperation { unit: *unit },
                self.span(id),
            ));
        }
        Ok(())
    }

    fn const_int(&mut self, id: NodeId) -> Result<Option<i64>, Diagnostic> {
        self.check(id)?;
        Ok(match self.out.info(id).detail {
            Detail::Const(Const::Int(i)) => Some(i),
            _ => None,
        })
    }

    fn pow(&mut self, base: NodeId, exp: NodeId) -> Result<Ty, Diagnostic> {
        let (u, _) = self.number(base)?;
        self.not_affine(&u, base)?;
        if u.is_dimensionless() {
            let (ue, _) = self.number(exp)?;
            self.align(exp, &ue, &Unit::ONE)?;
            return Ok(Ty::SCALAR);
        }
        let n = self
            .const_int(exp)?
            .and_then(|i| i8::try_from(i).ok())
            .ok_or(Diagnostic::new(
                ErrorKind::FractionalDimension { unit: u },
                self.span(exp),
            ))?;
        let unit = u
            .checked_powi(n)
            .map_err(|e| unit_error(e, self.span(base)))?;
        Ok(Ty::Number {
            unit,
            repr: Repr::Float,
        })
    }

    fn signal_arg(&mut self, id: NodeId) -> Result<SignalInfo, Diagnostic> {
        if self.out.ast.node(id) != Node::Path {
            return Err(Diagnostic::new(ErrorKind::ExpectedSignal, self.span(id)));
        }
        let info = self.signal(self.span(id))?;
        self.set(id, Ty::Bool, Detail::Signal(info));
        Ok(info)
    }

    fn duration_arg(&mut self, id: NodeId, window: bool) -> Result<Duration, Diagnostic> {
        let span = self.span(id);
        if !matches!(self.out.ast.node(id), Node::Literal { .. }) {
            return Err(Diagnostic::new(ErrorKind::ExpectedDuration, span));
        }
        let d = Duration::parse(self.text(span))
            .map_err(|e| Diagnostic::new(ErrorKind::Literal(e.kind), e.span.shifted(span.start)))?;
        if d.is_zero() {
            return Err(Diagnostic::new(ErrorKind::ZeroDuration, span));
        }
        if window && d > Duration::MAX_WINDOW {
            return Err(Diagnostic::new(ErrorKind::WindowTooLong, span));
        }
        self.set(id, Ty::SCALAR, Detail::Duration(d));
        Ok(d)
    }

    fn aggregate_arg(
        &mut self,
        id: NodeId,
        kind: AggregateKind,
    ) -> Result<Option<Unit>, Diagnostic> {
        if self.out.ast.node(id) != Node::Selector {
            return match kind {
                AggregateKind::Bool => self.boolean(id).map(|_| None),
                AggregateKind::Number(target) => {
                    let (u, _) = self.number(id)?;
                    if let Some(t) = target {
                        self.align(id, &u, &t)?;
                    }
                    Ok(Some(u))
                }
            };
        }
        let span = self.span(id);
        let first = self.out.elements_len;
        let mut unit = match kind {
            AggregateKind::Number(t) => t,
            AggregateKind::Bool => None,
        };
        let mut failure = None;
        let mut overflow = false;
        let mut found = [None; MAX_ELEMENTS];
        let mut n = 0usize;
        self.env.select(self.text(span), &mut |info| {
            match found.get_mut(n) {
                Some(slot) => *slot = Some(info),
                None => overflow = true,
            }
            n += 1;
        });
        if overflow || first as usize + n > MAX_ELEMENTS {
            return Err(Diagnostic::new(ErrorKind::TooManyElements, span));
        }
        if n == 0 {
            return Err(Diagnostic::new(ErrorKind::EmptySelection, span));
        }
        for info in found.iter().take(n).flatten() {
            let ty = Ty::of_signal(info)
                .ok_or(Diagnostic::new(ErrorKind::UnsupportedSignalType, span))?;
            let coerce = match (kind, ty) {
                (AggregateKind::Bool, Ty::Bool) => Conversion::IDENTITY,
                (AggregateKind::Bool, found) => {
                    failure = Some(self.mismatch(id, Expected::Bool, found));
                    break;
                }
                (AggregateKind::Number(_), Ty::Number { unit: u, .. }) => match unit {
                    None => {
                        unit = Some(u);
                        Conversion::IDENTITY
                    }
                    Some(t) => conversion(&u, &t, span)?,
                },
                (AggregateKind::Number(_), found) => {
                    failure = Some(self.mismatch(id, Expected::Number, found));
                    break;
                }
            };
            let index = self.out.elements_len as usize;
            if let Some(slot) = self.out.elements.get_mut(index) {
                *slot = Some(Element {
                    signal: *info,
                    coerce,
                });
                self.out.elements_len += 1;
            }
        }
        if let Some(e) = failure {
            return Err(e);
        }
        let count = self.out.elements_len - first;
        let ty = match unit {
            Some(u) => Ty::Number {
                unit: u,
                repr: Repr::Float,
            },
            None => Ty::Bool,
        };
        self.set(id, ty, Detail::Selector { first, count });
        Ok(unit)
    }
}

#[derive(Clone, Copy)]
enum AggregateKind {
    Bool,
    Number(Option<Unit>),
}

fn expected_like(ty: &Ty) -> Expected {
    match ty {
        Ty::Bool => Expected::Bool,
        Ty::Number { .. } => Expected::Number,
        Ty::Enum(_) => Expected::SameType,
    }
}
