use joker_units::Span;

use crate::limits::{MAX_ARGS, MAX_NODES};

/// The index of a node in an [`Ast`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u16);

impl NodeId {
    /// The index as `usize`.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-x`.
    Neg,
    /// `!x`.
    Not,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `||`.
    Or,
    /// `&&`.
    And,
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
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
}

impl BinaryOp {
    /// The operator as written.
    pub const fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Or => "||",
            BinaryOp::And => "&&",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
        }
    }

    /// True for the six comparison operators.
    pub const fn is_comparison(self) -> bool {
        matches!(
            self,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        )
    }
}

/// A node of the syntax tree. Names, paths and literals are kept as spans
/// into the source, so the tree never owns text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    /// A numeric literal with an optional unit; `unit_start` splits number and unit.
    Literal {
        /// Byte offset where the unit starts (the end of the span if none).
        unit_start: u32,
    },
    /// `true` or `false`.
    Bool(bool),
    /// A signal path.
    Path,
    /// A selector with `*` or `**`, only valid as an aggregation argument.
    Selector,
    /// An enum variant, `Enum.variant`.
    EnumVariant,
    /// A unary operation.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: NodeId,
    },
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: NodeId,
        /// Right operand.
        rhs: NodeId,
    },
    /// `if cond then a else b`.
    If {
        /// The condition.
        cond: NodeId,
        /// The value when the condition holds.
        then: NodeId,
        /// The value otherwise.
        otherwise: NodeId,
    },
    /// A function call; `name` is the span of the function name and the
    /// arguments are `args[first..first + count]` of the [`Ast`].
    Call {
        /// Span of the function name.
        name: Span,
        /// Index of the first argument in the argument list.
        first: u16,
        /// Number of arguments.
        count: u16,
    },
}

/// A syntax tree with fixed capacity: nodes and call arguments live in
/// inline arrays, so building one never allocates.
#[derive(Debug, Clone)]
pub struct Ast {
    nodes: [Node; MAX_NODES],
    spans: [Span; MAX_NODES],
    len: u16,
    args: [NodeId; MAX_ARGS],
    args_len: u16,
    root: NodeId,
}

impl Ast {
    pub(crate) fn empty() -> Ast {
        Ast {
            nodes: [Node::Bool(false); MAX_NODES],
            spans: [Span::default(); MAX_NODES],
            len: 0,
            args: [NodeId(0); MAX_ARGS],
            args_len: 0,
            root: NodeId(0),
        }
    }

    pub(crate) fn push(&mut self, node: Node, span: Span) -> Option<NodeId> {
        let id = self.len;
        *self.nodes.get_mut(id as usize)? = node;
        *self.spans.get_mut(id as usize)? = span;
        self.len = id.checked_add(1)?;
        Some(NodeId(id))
    }

    pub(crate) fn push_args(&mut self, ids: &[NodeId]) -> Option<(u16, u16)> {
        let first = self.args_len;
        let count = u16::try_from(ids.len()).ok()?;
        let end = first.checked_add(count)?;
        self.args
            .get_mut(first as usize..end as usize)?
            .copy_from_slice(ids);
        self.args_len = end;
        Some((first, count))
    }

    pub(crate) fn set_root(&mut self, root: NodeId) {
        self.root = root;
    }

    /// The root node.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// True when the tree has no node (never the case after a successful parse).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The node with this id.
    pub fn node(&self, id: NodeId) -> Node {
        self.nodes
            .get(id.index())
            .copied()
            .unwrap_or(Node::Bool(false))
    }

    /// The source span of a node.
    pub fn span(&self, id: NodeId) -> Span {
        self.spans.get(id.index()).copied().unwrap_or_default()
    }

    /// The arguments of a call node.
    pub fn args(&self, first: u16, count: u16) -> &[NodeId] {
        let end = first as usize + count as usize;
        self.args.get(first as usize..end).unwrap_or(&[])
    }
}
