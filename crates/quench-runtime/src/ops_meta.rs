#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    Ordinary,
    Arrow,
    Generator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionStrictness {
    Sloppy,
    Strict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyDefinitionKind {
    Data,
    Get,
    Set,
}

/// Opaque identity for a host realm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RealmId(u64);

impl RealmId {
    pub const ROOT: Self = Self(0);

    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Host capabilities that may be attached to a particular realm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HostCapabilityKind {
    GetGlobal,
    CreateRealm,
    EvalScript,
    DetachArrayBuffer,
    Agent,
    AgentStart,
    AgentBroadcast,
    AgentReport,
    AgentGetReport,
    AgentLeaving,
    AgentReceiveBroadcast,
    AgentSleep,
    AgentTryYield,
    AgentTrySleep,
    AgentSetTimeout,
    AgentMonotonicNow,
}

/// Non-JavaScript capability descriptor; exposure is owned by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostCapabilityRef {
    pub realm: RealmId,
    pub kind: HostCapabilityKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Exponentiate,
    /// Numeric increment/decrement addition used by `++`/`--`: ToNumeric the
    /// operand and add one in the operand's own type (never string-concats).
    NumericAdd,
    NumericSubtract,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    ShiftLeft,
    ShiftRight,
    ShiftRightZeroFill,
    Instanceof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Plus,
    Minus,
    Not,
    BitwiseNot,
    Void,
    Typeof,
    ToString,
    ToNumeric,
    Delete,
    IsNullish,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    String(String),
    BigInt(String),
    Null,
    Undefined,
}
