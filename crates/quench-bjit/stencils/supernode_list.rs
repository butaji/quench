pub static FAMILIES: &[(&str, Family)] = &[
    ("Binary", Family::Value),
    ("NumericAdd", Family::Value),
    ("NumericMultiply", Family::Value),
    ("JumpBinaryFalse", Family::Branch),
    ("GetIndex", Family::Index),
];

pub static SUPERNODES: &[Supernode] = &[
    Supernode { pattern: "NumericMultiply(*) ; NumericAdd(+) [0>1]", feeds: &[Operand::B], places: [VALUE_FIRST, VALUE_SECOND], evidence: "a + b * c: miner row for this pattern" },
    Supernode { pattern: "NumericAdd(+) ; NumericAdd(+) [0>1]", feeds: &[Operand::B], places: [VALUE_FIRST, VALUE_SECOND], evidence: "a + b + c: miner row for this pattern" },
    Supernode { pattern: "NumericMultiply(*) ; Binary(-) [0>1]", feeds: &[Operand::B], places: [VALUE_FIRST, VALUE_SECOND], evidence: "a - b * c: miner row for this pattern" },
    Supernode { pattern: "Binary(>>) ; Binary(&) [0>1]", feeds: &[Operand::B], places: [VALUE_FIRST, VALUE_SECOND], evidence: "(a >> b) & c: miner row for this pattern" },
    Supernode { pattern: "NumericAdd(+) ; JumpBinaryFalse(<) [0>1]", feeds: &[Operand::A, Operand::B], places: [VALUE_FIRST, BRANCH_SECOND], evidence: "a + b < c branch: miner row for this pattern" },
    Supernode { pattern: "GetIndex ; NumericAdd(+) [0>1]", feeds: &[Operand::B], places: [INDEX_FIRST, VALUE_SECOND], evidence: "a + array[i]: miner row for this pattern" },
    Supernode { pattern: "GetIndex ; JumpBinaryFalse(<) [0>1]", feeds: &[Operand::A, Operand::B], places: [INDEX_FIRST, BRANCH_SECOND], evidence: "array[i] < c branch: miner row for this pattern" },
];
