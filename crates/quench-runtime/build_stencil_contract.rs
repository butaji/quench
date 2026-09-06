#[derive(Clone, Copy, Debug)]
pub(crate) struct RegionDeclaration {
    pub(crate) name: &'static str,
    pub(crate) operations: &'static [&'static str],
    pub(crate) abi: DeclAbi,
    pub(crate) x86_bytes: &'static [u8],
    pub(crate) aarch64_bytes: &'static [u8],
    pub(crate) portable_bytes: &'static [u8],
    pub(crate) holes: &'static [(u16, usize, &'static str)],
    pub(crate) aarch64_holes: &'static [(u16, usize, &'static str)],
    pub(crate) entry: u32,
    pub(crate) external_entries: &'static [u32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeclAbi {
    ScalarF64Binary,
    ScalarF64Unary,
    ScalarF64x3,
    TaggedWord,
    ConstantWord,
    ScalarBool,
    ScalarWordBool,
    ScalarWordPairBool,
    ScalarI32,
    ScalarU32,
    Bridge,
    ArrayKernel,
    ArrayNumericLoop,
    CompareBranch,
    PropertyGuard,
    PropertyWriteGuard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecipeComposition {
    Whole,
    FallthroughReturn,
    AddChain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalOperandField {
    A,
    B,
    C,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalOperand {
    pub(crate) operation: u8,
    pub(crate) field: PhysicalOperandField,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalBindingValue {
    Operand(PhysicalOperand),
    RegionStart,
    RegionEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalBinding {
    Equal(PhysicalBindingValue, PhysicalBindingValue),
    AllDistinct(&'static [PhysicalOperand]),
}

impl PhysicalBinding {
    pub(crate) const fn valid_for(self, operation_count: usize) -> bool {
        match self {
            Self::Equal(left, right) => {
                left.valid_for(operation_count) && right.valid_for(operation_count)
            }
            Self::AllDistinct(operands) => {
                if operands.len() < 2 {
                    return false;
                }
                let mut index = 0;
                while index < operands.len() {
                    if operands[index].operation as usize >= operation_count {
                        return false;
                    }
                    index += 1;
                }
                true
            }
        }
    }
}

impl PhysicalBindingValue {
    const fn valid_for(self, operation_count: usize) -> bool {
        match self {
            Self::Operand(operand) => (operand.operation as usize) < operation_count,
            Self::RegionStart | Self::RegionEnd => true,
        }
    }
}

const fn operand(operation: u8, field: PhysicalOperandField) -> PhysicalOperand {
    PhysicalOperand { operation, field }
}

const fn value(operation: u8, field: PhysicalOperandField) -> PhysicalBindingValue {
    PhysicalBindingValue::Operand(operand(operation, field))
}

const fn equal(left: PhysicalBindingValue, right: PhysicalBindingValue) -> PhysicalBinding {
    PhysicalBinding::Equal(left, right)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AssemblyContinuation {
    pub(crate) head_name: &'static str,
    pub(crate) tail_name: &'static str,
    pub(crate) target: &'static str,
}

macro_rules! assembly_continuation {
    () => {
        None
    };
    (($head:literal, $tail:literal, $target:literal)) => {
        Some(AssemblyContinuation {
            head_name: $head,
            tail_name: $tail,
            target: $target,
        })
    };
}

macro_rules! recipe_composition {
    () => {
        RecipeComposition::Whole
    };
    ($composition:ident) => {
        RecipeComposition::$composition
    };
}

macro_rules! rust_leaf_catalog {
    ($( $variant:ident {
        name: $name:literal, abi: $abi:ident, ops: [$($op:literal),+],
        params: $params:literal, result: $result:literal, body: $body:literal
        $(, composition: $composition:ident)?
    } ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum RustLeafRecipe { $( $variant ),+ }

        impl RustLeafRecipe {
            pub(crate) const fn expression(self) -> &'static str {
                match self { $( Self::$variant => $body ),+ }
            }

            pub(crate) const fn parameters(self) -> &'static str {
                match self { $( Self::$variant => $params ),+ }
            }

            pub(crate) const fn result(self) -> &'static str {
                match self { $( Self::$variant => $result ),+ }
            }

            pub(crate) const fn composition(self) -> RecipeComposition {
                match self {
                    $( Self::$variant => recipe_composition!($($composition)?) ),+
                }
            }

            const fn name(self) -> &'static str {
                match self { $( Self::$variant => $name ),+ }
            }

            const fn abi(self) -> DeclAbi {
                match self { $( Self::$variant => DeclAbi::$abi ),+ }
            }

            const fn operations(self) -> &'static [&'static str] {
                match self { $( Self::$variant => &[$($op),+]),+ }
            }

            fn matches(self, declaration: &RegionDeclaration) -> bool {
                self.name() == declaration.name
                    && self.abi() == declaration.abi
                    && self.operations() == declaration.operations
            }
        }

        const RUST_LEAF_RECIPES: &[RustLeafRecipe] = &[$(RustLeafRecipe::$variant),+];
    };
}

rust_leaf_catalog! {
    Add { name: "loop", abi: ScalarF64Binary, ops: ["Add", "Return"], params: "a: f64, b: f64", result: "f64", body: "a + b" },
    Sub { name: "subtract", abi: ScalarF64Binary, ops: ["Sub", "Return"], params: "a: f64, b: f64", result: "f64", body: "a - b" },
    Mul { name: "multiply", abi: ScalarF64Binary, ops: ["Mul", "Return"], params: "a: f64, b: f64", result: "f64", body: "a * b" },
    Div { name: "divide", abi: ScalarF64Binary, ops: ["Div", "Return"], params: "a: f64, b: f64", result: "f64", body: "a / b" },
    AddConst { name: "add_const", abi: ScalarF64Binary, ops: ["AddConst", "Return"], params: "a: f64, b: f64", result: "f64", body: "a + b" },
    Negate { name: "negate", abi: ScalarF64Unary, ops: ["Unary", "Return"], params: "a: f64", result: "f64", body: "-a" },
    Increment { name: "increment", abi: ScalarF64Binary, ops: ["IncI", "Return"], params: "a: f64, _unused: f64", result: "f64", body: "a + 1.0" },
    Equal { name: "compare_equal", abi: ScalarBool, ops: ["Binary", "Return"], params: "a: f64, b: f64", result: "bool", body: "a == b" },
    NotEqual { name: "compare_not_equal", abi: ScalarBool, ops: ["Binary", "Return"], params: "a: f64, b: f64", result: "bool", body: "a != b" },
    Less { name: "compare_less", abi: ScalarBool, ops: ["Binary", "Return"], params: "a: f64, b: f64", result: "bool", body: "a < b" },
    LessEqual { name: "compare_less_equal", abi: ScalarBool, ops: ["Binary", "Return"], params: "a: f64, b: f64", result: "bool", body: "a <= b" },
    Greater { name: "compare_greater", abi: ScalarBool, ops: ["Binary", "Return"], params: "a: f64, b: f64", result: "bool", body: "a > b" },
    GreaterEqual { name: "compare_greater_equal", abi: ScalarBool, ops: ["Binary", "Return"], params: "a: f64, b: f64", result: "bool", body: "a >= b" },
    WordEqual { name: "compare_equal_word", abi: ScalarWordPairBool, ops: ["Binary", "Return"], params: "a: u64, b: u64", result: "bool", body: "a == b" },
    WordNotEqual { name: "compare_not_equal_word", abi: ScalarWordPairBool, ops: ["Binary", "Return"], params: "a: u64, b: u64", result: "bool", body: "a != b" },
    BitAnd { name: "bitwise_and", abi: ScalarI32, ops: ["Binary", "Return"], params: "a: i32, b: i32", result: "i32", body: "a & b" },
    BitOr { name: "bitwise_or", abi: ScalarI32, ops: ["Binary", "Return"], params: "a: i32, b: i32", result: "i32", body: "a | b" },
    BitXor { name: "bitwise_xor", abi: ScalarI32, ops: ["Binary", "Return"], params: "a: i32, b: i32", result: "i32", body: "a ^ b" },
    ShiftLeft { name: "shift_left", abi: ScalarI32, ops: ["Binary", "Return"], params: "a: i32, b: i32", result: "i32", body: "a.wrapping_shl((b as u32) & 31)" },
    ShiftRight { name: "shift_right", abi: ScalarI32, ops: ["Binary", "Return"], params: "a: i32, b: i32", result: "i32", body: "a >> ((b as u32) & 31)" },
    ShiftRightZero { name: "shift_right_zero", abi: ScalarU32, ops: ["Binary", "Return"], params: "a: i32, b: i32", result: "u32", body: "(a as u32) >> ((b as u32) & 31)" },
    BitNot { name: "bitwise_not", abi: ScalarI32, ops: ["Unary", "Return"], params: "a: i32", result: "i32", body: "!a" },
    TruthyNumber { name: "truthy_number", abi: ScalarBool, ops: ["JumpIfFalse"], params: "a: f64", result: "bool", body: "a != 0.0 && !a.is_nan()" },
}

pub(crate) fn rust_leaf_recipe(declaration: &RegionDeclaration) -> Option<RustLeafRecipe> {
    RUST_LEAF_RECIPES
        .iter()
        .copied()
        .find(|recipe| recipe.matches(declaration))
}

macro_rules! rust_assembly_catalog {
    ($( $variant:ident {
        name: $name:literal, abi: $abi:ident, ops: [$($op:literal),+],
        holes: [$($hole:expr),*]
        $(, bindings: $bindings:expr)?
        $(, continuation: { head: $head:literal, tail: $tail:literal, target: $target:literal })?
        $(, composition: $composition:ident)?
    } ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum RustAssemblyRecipe { $( $variant ),+ }

        impl RustAssemblyRecipe {
            pub(crate) const fn name(self) -> &'static str {
                match self { $( Self::$variant => $name ),+ }
            }

            pub(crate) const fn composition(self) -> RecipeComposition {
                match self {
                    $( Self::$variant => recipe_composition!($($composition)?) ),+
                }
            }

            pub(crate) const fn continuation(self) -> Option<AssemblyContinuation> {
                match self {
                    $( Self::$variant => assembly_continuation!($(($head, $tail, $target))?) ),+
                }
            }

            pub(crate) const fn bindings(self) -> &'static [PhysicalBinding] {
                match self {
                    $( Self::$variant => rust_assembly_bindings!($($bindings)?), )+
                }
            }

            fn matches(self, declaration: &RegionDeclaration) -> bool {
                match self {
                    $( Self::$variant => declaration.name == $name
                        && declaration.abi == DeclAbi::$abi
                        && declaration.operations == &[$($op),+]
                        && declaration.aarch64_holes == &[$($hole),*], )+
                }
            }
        }

        const RUST_ASSEMBLY_RECIPES: &[RustAssemblyRecipe] =
            &[$(RustAssemblyRecipe::$variant),+];
    };
}

macro_rules! rust_assembly_bindings {
    () => {
        &[]
    };
    ($bindings:expr) => {
        $bindings
    };
}

rust_assembly_catalog! {
    Fallthrough {
        name: "fallthrough", abi: ScalarF64Binary,
        ops: ["Add", "Return"], holes: [(4, 4, "Branch26"), (8, 4, "Branch26")],
        continuation: { head: "fallthrough_head", tail: "fallthrough_tail", target: "q_fallthrough_tail" },
        composition: FallthroughReturn
    },
    AddChain {
        name: "add_chain", abi: ScalarF64x3,
        ops: ["Add", "Add"], holes: [(4, 4, "Branch26")],
        continuation: { head: "add_chain_head", tail: "add_chain_tail", target: "q_add_chain_tail" },
        composition: AddChain
    },
    CompareEqualBranch {
        name: "compare_equal_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], holes: []
    },
    CompareNotEqualBranch {
        name: "compare_not_equal_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], holes: []
    },
    CompareLessBranch {
        name: "compare_less_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], holes: []
    },
    CompareLessEqualBranch {
        name: "compare_less_equal_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], holes: []
    },
    CompareGreaterBranch {
        name: "compare_greater_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], holes: []
    },
    CompareGreaterEqualBranch {
        name: "compare_greater_equal_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], holes: []
    },
    ArrayNumericLoop {
        name: "array_numeric_loop", abi: ArrayNumericLoop,
        ops: ["LoadLocal", "LoadConst", "Binary", "JumpIfFalse", "LoadLocal",
            "Move", "LoadLocal", "Move", "LoadLocal", "Slow", "LoadLocal",
            "AGetI", "AddConst", "ASetI", "Move", "LoadLocal", "AddConst",
            "StoreLocal", "Jump"], holes: [],
        bindings: &ARRAY_NUMERIC_LOOP_BINDINGS
    },
    Property { name: "property", abi: PropertyGuard, ops: ["GetN"], holes: [] },
    PrototypeProperty { name: "prototype_property", abi: PropertyGuard, ops: ["GetN"], holes: [] },
    StoreProperty { name: "store_property", abi: PropertyWriteGuard, ops: ["SetN"], holes: [] },
    ArrayGetNumber { name: "array_get_number", abi: ArrayKernel, ops: ["AGetI"], holes: [] },
    ArraySetNumber { name: "array_set_number", abi: ArrayKernel, ops: ["ASetI"], holes: [] },
    ArrayGetIncNumber { name: "array_get_inc_number", abi: ArrayKernel, ops: ["AGetIInc"], holes: [] },
    ArrayNumericUpdate { name: "array_numeric_update", abi: ArrayKernel, ops: ["AGetI", "Add", "ASetI"], holes: [] },
    ArrayNumericUpdateConst { name: "array_numeric_update_const", abi: ArrayKernel, ops: ["AGetI", "AddConst", "ASetI"], holes: [] },
    ArrayLoopBody { name: "array_loop_body", abi: ArrayKernel, ops: ["LoadLocalChecked", "AGetI", "Add", "ASetI", "Return"], holes: [] },
    Move { name: "move", abi: TaggedWord, ops: ["Move"], holes: [] },
    LoadLocal { name: "load_local", abi: TaggedWord, ops: ["LoadLocal"], holes: [] },
    StoreLocal { name: "store_local", abi: TaggedWord, ops: ["StoreLocal"], holes: [] },
    TruthyPointer { name: "truthy_pointer_word", abi: ScalarWordBool, ops: ["JumpIfFalse"], holes: [] },
    LoadConst { name: "load_const", abi: ConstantWord, ops: ["LoadConst", "Return"], holes: [(8, 8, "Literal64")] },
    NullishWord { name: "nullish_word", abi: ScalarWordBool, ops: ["Unary", "Return"], holes: [(24, 8, "Literal64")] },
    TruthyWord { name: "truthy_word", abi: ScalarWordBool, ops: ["JumpIfFalse"], holes: [(16, 8, "Literal64")] },
}

use PhysicalBindingValue::{RegionEnd, RegionStart};
use PhysicalOperandField::{A, B, C};

const ARRAY_NUMERIC_LOOP_DISTINCT: &[PhysicalOperand] = &[
    operand(0, A),
    operand(1, A),
    operand(4, A),
    operand(6, A),
    operand(8, A),
    operand(10, A),
    operand(11, A),
    operand(12, A),
    operand(15, A),
    operand(16, A),
];

const ARRAY_NUMERIC_LOOP_BINDINGS: &[PhysicalBinding] = &[
    equal(value(2, A), value(3, A)),
    equal(value(2, B), value(0, A)),
    equal(value(2, C), value(1, A)),
    equal(value(3, B), RegionEnd),
    equal(value(18, A), RegionStart),
    equal(value(4, B), value(8, B)),
    equal(value(8, A), value(11, B)),
    equal(value(6, B), value(15, B)),
    equal(value(6, B), value(10, B)),
    equal(value(0, A), value(17, A)),
    equal(value(16, B), value(15, A)),
    equal(value(17, B), value(16, A)),
    equal(value(7, A), value(13, A)),
    equal(value(11, A), value(12, B)),
    equal(value(12, A), value(13, C)),
    equal(value(14, B), value(12, A)),
    equal(value(5, B), value(4, A)),
    equal(value(7, B), value(5, A)),
    PhysicalBinding::AllDistinct(ARRAY_NUMERIC_LOOP_DISTINCT),
];

pub(crate) fn rust_assembly_recipe(declaration: &RegionDeclaration) -> Option<RustAssemblyRecipe> {
    RUST_ASSEMBLY_RECIPES
        .iter()
        .copied()
        .find(|recipe| recipe.matches(declaration))
}
