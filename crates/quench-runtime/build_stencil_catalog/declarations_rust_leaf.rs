rust_leaf_catalog! {
    Add {
        name: "loop", abi: ScalarF64Binary, ops: ["Add", "Return"],
        params: "a: f64, b: f64", result: "f64", body: "a + b",
        x86: &X86_LOOP_BYTES, aarch64: &AARCH64_LOOP_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    Sub {
        name: "subtract", abi: ScalarF64Binary, ops: ["Sub", "Return"],
        params: "a: f64, b: f64", result: "f64", body: "a - b",
        x86: &X86_SUBTRACT_BYTES, aarch64: &AARCH64_SUBTRACT_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    Mul {
        name: "multiply", abi: ScalarF64Binary, ops: ["Mul", "Return"],
        params: "a: f64, b: f64", result: "f64", body: "a * b",
        x86: &X86_MULTIPLY_BYTES, aarch64: &AARCH64_MULTIPLY_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    Div {
        name: "divide", abi: ScalarF64Binary, ops: ["Div", "Return"],
        params: "a: f64, b: f64", result: "f64", body: "a / b",
        x86: &X86_DIVIDE_BYTES, aarch64: &AARCH64_DIVIDE_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    AddConst {
        name: "add_const", abi: ScalarF64Binary, ops: ["AddConst", "Return"],
        params: "a: f64, b: f64", result: "f64", body: "a + b",
        x86: &X86_ADD_CONST_BYTES, aarch64: &AARCH64_ADD_CONST_BYTES,
        holes: &[(13, 8, "Literal64")], aarch64_holes: &[(16, 8, "Literal64")]
    },
    Negate {
        name: "negate", abi: ScalarF64Unary, ops: ["Unary", "Return"],
        params: "a: f64", result: "f64", body: "-a",
        x86: &X86_NEGATE_BYTES, aarch64: &AARCH64_NEGATE_BYTES,
        holes: &[(16, 8, "Literal64")], aarch64_holes: &[]
    },
    Increment {
        name: "increment", abi: ScalarF64Binary, ops: ["IncI", "Return"],
        params: "a: f64, _unused: f64", result: "f64", body: "a + 1.0",
        x86: &X86_ADD_CONST_BYTES, aarch64: &AARCH64_ADD_CONST_BYTES,
        holes: &[(13, 8, "Literal64")], aarch64_holes: &[(16, 8, "Literal64")]
    },
    Equal {
        name: "compare_equal", abi: ScalarBool, ops: ["Binary", "Return"],
        params: "a: f64, b: f64", result: "bool", body: "a == b",
        x86: &X86_COMPARE_EQUAL_BYTES, aarch64: &AARCH64_COMPARE_EQUAL_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    NotEqual {
        name: "compare_not_equal", abi: ScalarBool, ops: ["Binary", "Return"],
        params: "a: f64, b: f64", result: "bool", body: "a != b",
        x86: &X86_COMPARE_NOT_EQUAL_BYTES, aarch64: &AARCH64_COMPARE_NOT_EQUAL_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    Less {
        name: "compare_less", abi: ScalarBool, ops: ["Binary", "Return"],
        params: "a: f64, b: f64", result: "bool", body: "a < b",
        x86: &X86_COMPARE_LESS_BYTES, aarch64: &AARCH64_COMPARE_LESS_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    LessEqual {
        name: "compare_less_equal", abi: ScalarBool, ops: ["Binary", "Return"],
        params: "a: f64, b: f64", result: "bool", body: "a <= b",
        x86: &X86_COMPARE_LESS_EQUAL_BYTES,
        aarch64: &AARCH64_COMPARE_LESS_EQUAL_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    Greater {
        name: "compare_greater", abi: ScalarBool, ops: ["Binary", "Return"],
        params: "a: f64, b: f64", result: "bool", body: "a > b",
        x86: &X86_COMPARE_GREATER_BYTES, aarch64: &AARCH64_COMPARE_GREATER_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    GreaterEqual {
        name: "compare_greater_equal", abi: ScalarBool, ops: ["Binary", "Return"],
        params: "a: f64, b: f64", result: "bool", body: "a >= b",
        x86: &X86_COMPARE_GREATER_EQUAL_BYTES,
        aarch64: &AARCH64_COMPARE_GREATER_EQUAL_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    WordEqual {
        name: "compare_equal_word", abi: ScalarWordPairBool,
        ops: ["Binary", "Return"], params: "a: u64, b: u64",
        result: "bool", body: "a == b", x86: &X86_WORD_EQUAL_BYTES,
        aarch64: &AARCH64_WORD_EQUAL_BYTES, holes: &[], aarch64_holes: &[]
    },
    WordNotEqual {
        name: "compare_not_equal_word", abi: ScalarWordPairBool,
        ops: ["Binary", "Return"], params: "a: u64, b: u64",
        result: "bool", body: "a != b", x86: &X86_WORD_NOT_EQUAL_BYTES,
        aarch64: &AARCH64_WORD_NOT_EQUAL_BYTES, holes: &[], aarch64_holes: &[]
    },
    BitAnd {
        name: "bitwise_and", abi: ScalarI32, ops: ["Binary", "Return"],
        params: "a: i32, b: i32", result: "i32", body: "a & b",
        x86: &X86_BITWISE_AND_BYTES, aarch64: &AARCH64_BITWISE_AND_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    BitOr {
        name: "bitwise_or", abi: ScalarI32, ops: ["Binary", "Return"],
        params: "a: i32, b: i32", result: "i32", body: "a | b",
        x86: &X86_BITWISE_OR_BYTES, aarch64: &AARCH64_BITWISE_OR_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    BitXor {
        name: "bitwise_xor", abi: ScalarI32, ops: ["Binary", "Return"],
        params: "a: i32, b: i32", result: "i32", body: "a ^ b",
        x86: &X86_BITWISE_XOR_BYTES, aarch64: &AARCH64_BITWISE_XOR_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    ShiftLeft {
        name: "shift_left", abi: ScalarI32, ops: ["Binary", "Return"],
        params: "a: i32, b: i32", result: "i32",
        body: "a.wrapping_shl((b as u32) & 31)",
        x86: &X86_SHIFT_LEFT_BYTES, aarch64: &AARCH64_SHIFT_LEFT_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    ShiftRight {
        name: "shift_right", abi: ScalarI32, ops: ["Binary", "Return"],
        params: "a: i32, b: i32", result: "i32",
        body: "a >> ((b as u32) & 31)",
        x86: &X86_SHIFT_RIGHT_BYTES, aarch64: &AARCH64_SHIFT_RIGHT_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    ShiftRightZero {
        name: "shift_right_zero", abi: ScalarU32, ops: ["Binary", "Return"],
        params: "a: i32, b: i32", result: "u32",
        body: "(a as u32) >> ((b as u32) & 31)",
        x86: &X86_SHIFT_RIGHT_ZERO_BYTES,
        aarch64: &AARCH64_SHIFT_RIGHT_ZERO_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    BitNot {
        name: "bitwise_not", abi: ScalarI32, ops: ["Unary", "Return"],
        params: "a: i32", result: "i32", body: "!a",
        x86: &X86_BITWISE_NOT_BYTES, aarch64: &AARCH64_BITWISE_NOT_BYTES,
        holes: &[], aarch64_holes: &[]
    },
    TruthyNumber {
        name: "truthy_number", abi: ScalarBool, ops: ["JumpIfFalse"],
        params: "a: f64", result: "bool", body: "a != 0.0 && !a.is_nan()",
        x86: &X86_TRUTHY_NUMBER_BYTES, aarch64: &AARCH64_TRUTHY_NUMBER_BYTES,
        holes: &[], aarch64_holes: &[]
    },
}

pub(crate) fn rust_leaf_recipe(declaration: &RegionDeclaration) -> Option<RustLeafRecipe> {
    RUST_LEAF_DECLARATIONS
        .iter()
        .zip(RUST_LEAF_RECIPES)
        .find_map(|(candidate, recipe)| (candidate == declaration).then_some(*recipe))
}

#[cfg(test)]
pub(crate) fn rust_leaf_declaration(recipe: RustLeafRecipe) -> &'static RegionDeclaration {
    let index = RUST_LEAF_RECIPES
        .iter()
        .position(|candidate| *candidate == recipe)
        .expect("recipe belongs to canonical leaf catalog");
    &RUST_LEAF_DECLARATIONS[index]
}
