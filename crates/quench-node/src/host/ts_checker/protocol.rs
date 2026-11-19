// Shared by Quench and its type-checker worker (tools/quench-tsrs-worker), so a stale worker
// binary or facts file never installs facts: both sides compare these.
pub const PROTOCOL_VERSION: u32 = 3;
pub const TYPESCRIPT_VERSION: &str = "7.1.0-beta";
pub const TSRS_REVISION: &str = "e18fb617ee516917342dcda27063ab67acc79e49";
/// First line of a checked-facts file: `quench-ts-facts <protocol> <tsrs revision>`.
pub const FACTS_MAGIC: &str = "quench-ts-facts";
/// No checked fact: every language type (`quench_runtime` `DECLARED_ANY`).
pub const LANGUAGE_ANY: u16 = u16::MAX;
/// Language type bits, `1 << quench_runtime::ValueType as u16`.
pub const LANGUAGE_UNDEFINED: u16 = 1 << 0;
pub const LANGUAGE_NULL: u16 = 1 << 1;
pub const LANGUAGE_BOOLEAN: u16 = 1 << 2;
pub const LANGUAGE_NUMBER: u16 = 1 << 3;
pub const LANGUAGE_STRING: u16 = 1 << 4;
pub const LANGUAGE_SYMBOL: u16 = 1 << 5;
pub const LANGUAGE_BIGINT: u16 = 1 << 6;
pub const LANGUAGE_OBJECT: u16 = 1 << 7;
pub const LANGUAGE_FUNCTION: u16 = 1 << 8;
