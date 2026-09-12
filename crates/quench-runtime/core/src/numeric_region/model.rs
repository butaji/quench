use crate::Op;
use crate::dynbytecode::Register;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::ops::Add;

pub struct NumericDenseState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NumericUnary {
    Plus,
    Negate,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NumericBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    ShiftLeft,
    ShiftRight,
    ShiftRightUnsigned,
    BitOr,
    BitXor,
    BitAnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticPropertyAccess {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyValueKind {
    TriviallyCopyable,
    Number,
    DenseArray,
}

impl PropertyValueKind {
    pub const fn merge(self, demanded: Self) -> Option<Self> {
        match (self, demanded) {
            (Self::TriviallyCopyable, kind) | (kind, Self::TriviallyCopyable) => Some(kind),
            (Self::Number, Self::Number) => Some(Self::Number),
            (Self::DenseArray, Self::DenseArray) => Some(Self::DenseArray),
            (Self::Number, Self::DenseArray) | (Self::DenseArray, Self::Number) => None,
        }
    }
}

impl NumericBinary {
    pub(super) fn from_op(op: Op) -> Option<Self> {
        Some(match op {
            Op::Add => Self::Add,
            Op::Sub => Self::Subtract,
            Op::Mul => Self::Multiply,
            Op::Div => Self::Divide,
            Op::Eq => Self::Equal,
            Op::Ne => Self::NotEqual,
            Op::StrictEq => Self::StrictEqual,
            Op::StrictNe => Self::StrictNotEqual,
            Op::Lt => Self::Less,
            Op::Le => Self::LessEqual,
            Op::Gt => Self::Greater,
            Op::Ge => Self::GreaterEqual,
            Op::Shl => Self::ShiftLeft,
            Op::Shr => Self::ShiftRight,
            Op::Ushr => Self::ShiftRightUnsigned,
            Op::Or => Self::BitOr,
            Op::Xor => Self::BitXor,
            Op::And => Self::BitAnd,
            Op::Rem | Op::Pow => return None,
        })
    }

    pub const fn result_is_boolean(self) -> bool {
        matches!(
            self,
            Self::Equal
                | Self::NotEqual
                | Self::StrictEqual
                | Self::StrictNotEqual
                | Self::Less
                | Self::LessEqual
                | Self::Greater
                | Self::GreaterEqual
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionOp {
    /// A semantic operation erased by a quote-to-quote rewrite.  Its bytecode
    /// PC remains as a connector-only stencil because the current internal ABI
    /// advances the InlineSite cursor by one site at every boundary.
    Elided {
        pc: usize,
    },
    NumberLiteral {
        pc: usize,
        dst: Register,
        bits: u64,
    },
    ReadLocal {
        pc: usize,
        dst: Register,
        slot: usize,
    },
    ReadCaptured {
        pc: usize,
        dst: Register,
        name: String,
    },
    WriteLocal {
        pc: usize,
        slot: usize,
        src: Register,
    },
    Move {
        pc: usize,
        dst: Register,
        src: Register,
    },
    Unary {
        pc: usize,
        dst: Register,
        src: Register,
        kind: NumericUnary,
    },
    Binary {
        pc: usize,
        dst: Register,
        left: Register,
        right: Register,
        kind: NumericBinary,
    },
    ReadDense {
        pc: usize,
        dst: Register,
        object: Register,
        index: Register,
    },
    WriteDense {
        pc: usize,
        object: Register,
        index: Register,
        src: Register,
    },
    ReadStatic {
        pc: usize,
        dst: Register,
        object: Register,
        key: String,
    },
    WriteStatic {
        pc: usize,
        object: Register,
        key: String,
        src: Register,
    },
    Jump {
        pc: usize,
        target: usize,
    },
    JumpIfFalse {
        pc: usize,
        test: Register,
        target: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionNode {
    Empty,
    Op(RegionOp),
    Seq(Vec<RegionNode>),
    Trace {
        header: usize,
        exit: usize,
        body: Box<RegionNode>,
    },
}

pub struct Region<In, Out> {
    pub(super) node: RegionNode,
    states: PhantomData<fn(In) -> Out>,
}

impl<In, Out> Clone for Region<In, Out> {
    fn clone(&self) -> Self {
        Self::new(self.node.clone())
    }
}

impl<In, Out> std::fmt::Debug for Region<In, Out> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.node.fmt(formatter)
    }
}

impl<In, Out> PartialEq for Region<In, Out> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl<In, Out> Eq for Region<In, Out> {}

impl<In, Out> Region<In, Out> {
    pub(super) fn new(node: RegionNode) -> Self {
        Self {
            node,
            states: PhantomData,
        }
    }

    pub fn node(&self) -> &RegionNode {
        &self.node
    }

    pub fn compose<Next>(self, next: Region<Out, Next>) -> Region<In, Next> {
        Region::new(sequence(self.node, next.node))
    }
}

impl<First, Middle, Last> Add<Region<Middle, Last>> for Region<First, Middle> {
    type Output = Region<First, Last>;

    fn add(self, next: Region<Middle, Last>) -> Self::Output {
        self.compose(next)
    }
}

pub fn identity<State>() -> Region<State, State> {
    Region::new(RegionNode::Empty)
}

fn sequence(left: RegionNode, right: RegionNode) -> RegionNode {
    let mut parts = match left {
        RegionNode::Empty => Vec::new(),
        RegionNode::Seq(parts) => parts,
        node => vec![node],
    };
    match right {
        RegionNode::Empty => {}
        RegionNode::Seq(nodes) => parts.extend(nodes),
        node => parts.push(node),
    }
    match parts.len() {
        0 => RegionNode::Empty,
        1 => parts.pop().unwrap(),
        _ => RegionNode::Seq(parts),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuardSource {
    Local(usize),
    Captured(String),
    LiveIn(Register),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuardKind {
    Number,
    ArrayIndex,
    DenseArray,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionEffect {
    ReadLocal(usize),
    WriteLocal(usize),
    ReadCaptured(String),
    ReadDense(Register),
    WriteDense(Register),
    ReadStatic(Register),
    WriteStatic(Register),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyRequirement {
    pub pc: usize,
    pub source: GuardSource,
    pub key: String,
    pub access: StaticPropertyAccess,
    pub value_kind: PropertyValueKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RewriteStats {
    pub local_loads_forwarded: usize,
    pub captured_loads_reused: usize,
    pub literals_reused: usize,
    pub copies_propagated: usize,
    pub pure_expressions_reused: usize,
    pub heap_loads_reused: usize,
    pub local_stores_eliminated: usize,
}

impl RewriteStats {
    pub const fn eliminated_operations(self) -> usize {
        self.local_loads_forwarded
            + self.captured_loads_reused
            + self.literals_reused
            + self.copies_propagated
            + self.pure_expressions_reused
            + self.heap_loads_reused
            + self.local_stores_eliminated
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotedLoop {
    pub start: usize,
    pub end: usize,
    pub exit: usize,
    pub region: Region<NumericDenseState, NumericDenseState>,
    pub(super) requirements: Vec<(GuardSource, GuardKind)>,
    pub(super) internal_local_sources: BTreeMap<Register, Register>,
    pub(super) internal_number_loads: Vec<usize>,
    pub(super) proven_index_sites: Vec<usize>,
    pub(super) property_requirements: Vec<PropertyRequirement>,
    pub(super) rewrite_stats: RewriteStats,
}

impl QuotedLoop {
    pub fn requirements(&self) -> &[(GuardSource, GuardKind)] {
        &self.requirements
    }

    pub fn property_requirements(&self) -> &[PropertyRequirement] {
        &self.property_requirements
    }

    pub fn internal_number_loads(&self) -> &[usize] {
        &self.internal_number_loads
    }

    pub(super) fn internal_local_sources(&self) -> &BTreeMap<Register, Register> {
        &self.internal_local_sources
    }

    pub fn proven_index_sites(&self) -> &[usize] {
        &self.proven_index_sites
    }

    pub const fn rewrite_stats(&self) -> RewriteStats {
        self.rewrite_stats
    }

    pub fn trace_header(&self) -> Option<usize> {
        fn find(node: &RegionNode) -> Option<usize> {
            match node {
                RegionNode::Trace { header, .. } => Some(*header),
                RegionNode::Seq(parts) => parts.iter().find_map(find),
                RegionNode::Empty | RegionNode::Op(_) => None,
            }
        }
        find(self.region.node())
    }

    pub fn labels(&self) -> Vec<usize> {
        let mut labels = BTreeSet::from([self.start, self.exit]);
        visit_ops(self.region.node(), &mut |op| match op {
            RegionOp::Jump { target, .. } | RegionOp::JumpIfFalse { target, .. } => {
                labels.insert(*target);
            }
            _ => {}
        });
        labels.into_iter().collect()
    }

    pub fn effects(&self) -> Vec<RegionEffect> {
        let mut effects = Vec::new();
        visit_ops(self.region.node(), &mut |op| match op {
            RegionOp::ReadLocal { slot, .. } => effects.push(RegionEffect::ReadLocal(*slot)),
            RegionOp::WriteLocal { slot, .. } => effects.push(RegionEffect::WriteLocal(*slot)),
            RegionOp::ReadCaptured { name, .. } => {
                effects.push(RegionEffect::ReadCaptured(name.clone()));
            }
            RegionOp::ReadDense { object, .. } => {
                effects.push(RegionEffect::ReadDense(*object));
            }
            RegionOp::WriteDense { object, .. } => {
                effects.push(RegionEffect::WriteDense(*object));
            }
            RegionOp::ReadStatic { object, .. } => {
                effects.push(RegionEffect::ReadStatic(*object));
            }
            RegionOp::WriteStatic { object, .. } => {
                effects.push(RegionEffect::WriteStatic(*object));
            }
            RegionOp::Elided { .. } => {}
            _ => {}
        });
        effects
    }

    pub fn op_count(&self) -> usize {
        let mut count = 0;
        visit_ops(self.region.node(), &mut |_| count += 1);
        count
    }

    pub fn operations(&self) -> Vec<RegionOp> {
        let mut operations = Vec::new();
        visit_ops(self.region.node(), &mut |op| operations.push(op.clone()));
        operations
    }
}

pub(super) fn visit_ops(node: &RegionNode, visitor: &mut impl FnMut(&RegionOp)) {
    match node {
        RegionNode::Empty => {}
        RegionNode::Op(op) => visitor(op),
        RegionNode::Seq(parts) => {
            for part in parts {
                visit_ops(part, visitor);
            }
        }
        RegionNode::Trace { body, .. } => visit_ops(body, visitor),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    TooShort,
    NoDenseAccess,
    NoNumericOperation,
    UnsupportedOpcode { pc: usize, opcode: &'static str },
    NestedLoop { pc: usize, target: usize },
    ExternalEntry { pc: usize, target: usize },
    MultipleExits,
    EscapingTemporary(Register),
    TypeConflict(GuardSource),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedLoop {
    pub start: usize,
    pub end: usize,
    pub reason: RejectReason,
}

#[derive(Default)]
pub struct RegionAnalysis {
    pub loops: Vec<QuotedLoop>,
    pub rejected: Vec<RejectedLoop>,
}
