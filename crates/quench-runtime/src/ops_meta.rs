#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    Ordinary,
    Arrow,
    Generator,
    Method,
    ClassConstructor,
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
    /// Class `F.prototype`: writable false, enumerable false, configurable false.
    ClassPrototype,
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
    /// Host-only callable object with an [[IsHTMLDDA]] slot.
    IsHTMLDDA,
    /// Engine lifecycle notification consumed by an embedding host.
    /// This is not exposed as a JavaScript capability.
    PromiseHook,
    /// Capability-owned operation implemented by the embedding host.
    Custom(u16),
}

/// Non-JavaScript capability descriptor; exposure is owned by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostCapabilityRef {
    pub realm: RealmId,
    pub kind: HostCapabilityKind,
}

include!("../operator_catalog.rs");

/// Declare the complete compact binary alphabet once. The generated id and
/// decoder are consumed by compact IR and opcode selectors, so `ir.rs` does
/// not maintain a second hand-written operator/id table.
macro_rules! binary_op_catalog {
    ($($name:ident = $id:literal => $region:expr),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum BinaryOp {
            $($name = $id),+
        }

        impl BinaryOp {
            pub const COUNT: u8 = binary_op_catalog!(@last $($id),+);
            pub const ALL: &'static [Self] = &[$(Self::$name),+];

            #[inline(always)]
            pub const fn compact_id(self) -> u8 {
                self as u8
            }

            pub const fn from_compact_id(value: u8) -> Option<Self> {
                match value {
                    $($id => Some(Self::$name),)+
                    _ => None,
                }
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name),)+
                }
            }

            pub const fn region_name(self) -> Option<&'static str> {
                match self {
                    $(Self::$name => $region,)+
                }
            }
        }

        const _: () = {
            let mut index = 0;
            while index < BinaryOp::ALL.len() {
                assert!(BinaryOp::ALL[index].compact_id() == index as u8);
                index += 1;
            }
            assert!(BinaryOp::COUNT + 1 == BinaryOp::ALL.len() as u8);
        };
    };
    (@last $head:literal, $($tail:literal),+) => { binary_op_catalog!(@last $($tail),+) };
    (@last $last:literal) => { $last };
}

// Numeric increment/decrement addition used by `++`/`--`: ToNumeric the
// operand and add one in the operand's own type (never string-concats).
with_binary_operator_catalog!(binary_op_catalog);

macro_rules! unary_op_catalog {
    ($($name:ident = $id:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum UnaryOp {
            $($name = $id),+
        }

        impl UnaryOp {
            pub const COUNT: u8 = unary_op_catalog!(@last $($id),+);
            pub const ALL: &'static [Self] = &[$(Self::$name),+];

            #[inline(always)]
            pub const fn compact_id(self) -> u8 {
                self as u8
            }

            pub const fn from_compact_id(value: u8) -> Option<Self> {
                match value {
                    $($id => Some(Self::$name),)+
                    _ => None,
                }
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name),)+
                }
            }
        }

        const _: () = {
            let mut index = 0;
            while index < UnaryOp::ALL.len() {
                assert!(UnaryOp::ALL[index].compact_id() == index as u8);
                index += 1;
            }
            assert!(UnaryOp::COUNT + 1 == UnaryOp::ALL.len() as u8);
        };
    };
    (@last $head:literal, $($tail:literal),+) => { unary_op_catalog!(@last $($tail),+) };
    (@last $last:literal) => { $last };
}

with_unary_operator_catalog!(unary_op_catalog);

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    String(String),
    StringUnits(Vec<u16>),
    BigInt(String),
    Null,
    Undefined,
}
