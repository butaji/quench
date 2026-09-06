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

macro_rules! rust_leaf_catalog {
    ($( $variant:ident {
        name: $name:literal, abi: $abi:ident, ops: [$($op:literal),+],
        params: $params:literal, result: $result:literal, body: $body:literal
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
    AddChain { name: "add_chain", abi: ScalarF64x3, ops: ["Add", "Add"], params: "a: f64, b: f64, c: f64", result: "f64", body: "(a + b) + c" },
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
    } ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum RustAssemblyRecipe { $( $variant ),+ }

        impl RustAssemblyRecipe {
            pub(crate) const fn name(self) -> &'static str {
                match self { $( Self::$variant => $name ),+ }
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

rust_assembly_catalog! {
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
            "StoreLocal", "Jump"], holes: []
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

pub(crate) fn rust_assembly_recipe(declaration: &RegionDeclaration) -> Option<RustAssemblyRecipe> {
    RUST_ASSEMBLY_RECIPES
        .iter()
        .copied()
        .find(|recipe| recipe.matches(declaration))
}
