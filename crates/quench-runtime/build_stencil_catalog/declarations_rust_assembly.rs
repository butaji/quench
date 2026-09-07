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

const BOOL_BRANCH_LINKS: &[AssemblyControlLink] = &[
    AssemblyControlLink {
        offset: 4,
        width: 4,
        kind: "Branch26",
        target: "q_bool_branch_false",
        role: AssemblySuccessorRole::False,
    },
    AssemblyControlLink {
        offset: 8,
        width: 4,
        kind: "Branch26",
        target: "q_bool_branch_true",
        role: AssemblySuccessorRole::True,
    },
];

const TRUTHY_BOOL_BRANCH_LINKS: &[AssemblyControlLink] = &[
    AssemblyControlLink {
        offset: 12,
        width: 4,
        kind: "Branch26",
        target: "q_truthy_bool_branch_false",
        role: AssemblySuccessorRole::False,
    },
    AssemblyControlLink {
        offset: 16,
        width: 4,
        kind: "Branch26",
        target: "q_truthy_bool_branch_true",
        role: AssemblySuccessorRole::True,
    },
];

const TRUTHY_BOOL_BRANCH_HOLES: &[AssemblyPatchHole] = &[AssemblyPatchHole {
    offset: 24,
    width: 8,
    kind: "Literal64",
}];

const WORD_CONST_LINKS: &[AssemblyControlLink] = &[AssemblyControlLink {
    offset: 4,
    width: 4,
    kind: "Branch26",
    target: "q_word_const_fragment_next",
    role: AssemblySuccessorRole::Next,
}];

const WORD_CONST_HOLES: &[AssemblyPatchHole] = &[AssemblyPatchHole {
    offset: 8,
    width: 8,
    kind: "Literal64",
}];

include!("../build_stencil_outputs.rs");

rust_assembly_catalog! {
    Fallthrough {
        name: "fallthrough", abi: ScalarF64Binary, ops: ["Add", "Return"],
        x86: &X86_FALLTHROUGH_BYTES, aarch64: &AARCH64_FALLTHROUGH_BYTES,
        x86_holes: &[(5, 4, "Rel32")],
        aarch64_holes: &[(4, 4, "Branch26"), (8, 4, "Branch26")],
        continuation: { head: "fallthrough_head", tail: "fallthrough_tail", target: "q_fallthrough_tail" },
        internal_abi: F64AccumulatorD0AddD1,
        composition: LinkedFragments
    },
    SubFallthrough {
        name: "sub_fallthrough", abi: ScalarF64Binary, ops: ["Sub", "Return"],
        x86: &X86_SUB_FALLTHROUGH_BYTES, aarch64: &AARCH64_SUB_FALLTHROUGH_BYTES,
        x86_holes: &[(5, 4, "Rel32")], aarch64_holes: &[(4, 4, "Branch26")],
        continuation: { head: "sub_fallthrough_head", tail: "fallthrough_tail", target: "q_fallthrough_tail" },
        internal_abi: F64AccumulatorD0AddD1,
        composition: LinkedFragments
    },
    MulFallthrough {
        name: "mul_fallthrough", abi: ScalarF64Binary, ops: ["Mul", "Return"],
        x86: &X86_MUL_FALLTHROUGH_BYTES, aarch64: &AARCH64_MUL_FALLTHROUGH_BYTES,
        x86_holes: &[(5, 4, "Rel32")], aarch64_holes: &[(4, 4, "Branch26")],
        continuation: { head: "mul_fallthrough_head", tail: "fallthrough_tail", target: "q_fallthrough_tail" },
        internal_abi: F64AccumulatorD0AddD1,
        composition: LinkedFragments
    },
    DivFallthrough {
        name: "div_fallthrough", abi: ScalarF64Binary, ops: ["Div", "Return"],
        x86: &X86_DIV_FALLTHROUGH_BYTES, aarch64: &AARCH64_DIV_FALLTHROUGH_BYTES,
        x86_holes: &[(5, 4, "Rel32")], aarch64_holes: &[(4, 4, "Branch26")],
        continuation: { head: "div_fallthrough_head", tail: "fallthrough_tail", target: "q_fallthrough_tail" },
        internal_abi: F64AccumulatorD0AddD1,
        composition: LinkedFragments
    },
    AddChain {
        name: "add_chain", abi: ScalarF64x3, ops: ["Add", "Add"],
        x86: &X86_ADD_CHAIN_BYTES, aarch64: &AARCH64_ADD_CHAIN_BYTES,
        x86_holes: &[(5, 4, "Rel32")], aarch64_holes: &[(4, 4, "Branch26")],
        continuation: { head: "add_chain_head", tail: "add_chain_tail", target: "q_add_chain_tail" },
        internal_abi: F64AccumulatorD0ThenD2,
        composition: LinkedFragments
    },
    BoolBranch {
        name: "bool_branch", abi: ScalarWordBool, ops: ["JumpIfFalse"],
        x86: &[], aarch64: &[],
        x86_holes: &[], aarch64_holes: &[],
        control_links: BOOL_BRANCH_LINKS,
        internal_abi: WordX0,
        composition: ControlFragment
    },
    TruthyBoolBranch {
        name: "truthy_bool_branch", abi: ScalarWordBool, ops: ["JumpIfFalse"],
        x86: &[], aarch64: &[],
        x86_holes: &[], aarch64_holes: &[],
        control_links: TRUTHY_BOOL_BRANCH_LINKS,
        patch_holes: TRUTHY_BOOL_BRANCH_HOLES,
        internal_abi: WordX0,
        composition: ControlFragment
    },
    ReturnWord {
        name: "return_word", abi: ScalarWordBool, ops: ["Return"],
        x86: &[], aarch64: &[],
        x86_holes: &[], aarch64_holes: &[],
        internal_abi: WordX0,
        composition: Whole
    },
    WordConstFragment {
        name: "word_const_fragment", abi: ScalarWordBool, ops: ["LoadConst"],
        x86: &[], aarch64: &[],
        x86_holes: &[], aarch64_holes: &[],
        control_links: WORD_CONST_LINKS,
        patch_holes: WORD_CONST_HOLES,
        internal_abi: WordX0,
        composition: ControlFragment
    },
    CompareEqualBranch {
        name: "compare_equal_branch", abi: CompareBranch, ops: ["Binary", "JumpIfFalse"],
        x86: &[0xC3], aarch64: &AARCH64_COMPARE_EQUAL_BRANCH_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    CompareNotEqualBranch {
        name: "compare_not_equal_branch", abi: CompareBranch, ops: ["Binary", "JumpIfFalse"],
        x86: &[0xC3], aarch64: &AARCH64_COMPARE_NOT_EQUAL_BRANCH_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    CompareLessBranch {
        name: "compare_less_branch", abi: CompareBranch, ops: ["Binary", "JumpIfFalse"],
        x86: &[0xC3], aarch64: &AARCH64_COMPARE_LESS_BRANCH_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    CompareLessEqualBranch {
        name: "compare_less_equal_branch", abi: CompareBranch, ops: ["Binary", "JumpIfFalse"],
        x86: &[0xC3], aarch64: &AARCH64_COMPARE_LESS_EQUAL_BRANCH_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    CompareGreaterBranch {
        name: "compare_greater_branch", abi: CompareBranch, ops: ["Binary", "JumpIfFalse"],
        x86: &[0xC3], aarch64: &AARCH64_COMPARE_GREATER_BRANCH_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    CompareGreaterEqualBranch {
        name: "compare_greater_equal_branch", abi: CompareBranch,
        ops: ["Binary", "JumpIfFalse"], x86: &[0xC3],
        aarch64: &AARCH64_COMPARE_GREATER_EQUAL_BRANCH_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    ArrayNumericLoop {
        name: "array_numeric_loop", abi: ArrayNumericLoop,
        ops: ["LoadLocal", "LoadConst", "Binary", "JumpIfFalse", "LoadLocal",
            "Move", "LoadLocal", "Move", "LoadLocal", "Slow", "LoadLocal",
            "AGetI", "AddConst", "ASetI", "Move", "LoadLocal", "AddConst",
            "StoreLocal", "Jump"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_LOOP_BYTES,
        x86_holes: &[], aarch64_holes: &[],
        bindings: &ARRAY_NUMERIC_LOOP_BINDINGS, outputs: &ARRAY_NUMERIC_LOOP_OUTPUTS
    },
    Property {
        name: "property", abi: PropertyGuard, ops: ["GetN"],
        x86: &X86_PROPERTY_BYTES, aarch64: &AARCH64_PROPERTY_GUARD_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    PrototypeProperty {
        name: "prototype_property", abi: PropertyGuard, ops: ["GetN"],
        x86: &[0xC3], aarch64: &AARCH64_PROTOTYPE_PROPERTY_GUARD_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    StoreProperty {
        name: "store_property", abi: PropertyWriteGuard, ops: ["SetN"],
        x86: &X86_PROPERTY_WRITE_BYTES, aarch64: &AARCH64_PROPERTY_WRITE_GUARD_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    ArrayGetNumber {
        name: "array_get_number", abi: ArrayKernel, ops: ["AGetI"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_GET_NUMBER_BYTES,
        x86_holes: &[], aarch64_holes: &[], outputs: &ARRAY_GET_OUTPUTS
    },
    ArraySetNumber {
        name: "array_set_number", abi: ArrayKernel, ops: ["ASetI"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_SET_NUMBER_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    ArrayGetIncNumber {
        name: "array_get_inc_number", abi: ArrayKernel, ops: ["AGetIInc"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_GET_INC_NUMBER_BYTES,
        x86_holes: &[], aarch64_holes: &[], outputs: &ARRAY_GET_INC_OUTPUTS
    },
    ArrayNumericUpdate {
        name: "array_numeric_update", abi: ArrayKernel, ops: ["AGetI", "Add", "ASetI"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_KERNEL_BYTES,
        x86_holes: &[], aarch64_holes: &[], outputs: &ARRAY_UPDATE_OUTPUTS
    },
    ArrayNumericUpdateConst {
        name: "array_numeric_update_const", abi: ArrayKernel,
        ops: ["AGetI", "AddConst", "ASetI"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_KERNEL_BYTES,
        x86_holes: &[], aarch64_holes: &[], outputs: &ARRAY_UPDATE_OUTPUTS
    },
    ArrayLoopBody {
        name: "array_loop_body", abi: ArrayKernel,
        ops: ["LoadLocalChecked", "AGetI", "Add", "ASetI", "Return"],
        x86: &X86_DISPATCH_BYTES, aarch64: &AARCH64_ARRAY_KERNEL_BYTES,
        x86_holes: &[(2, 8, "Ptr64")], aarch64_holes: &[],
        outputs: &ARRAY_LOOP_BODY_OUTPUTS
    },
    Move {
        name: "move", abi: TaggedWord, ops: ["Move"],
        x86: &X86_MOVE_BYTES, aarch64: &AARCH64_MOVE_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    LoadLocal {
        name: "load_local", abi: TaggedWord, ops: ["LoadLocal"],
        x86: &X86_MOVE_BYTES, aarch64: &AARCH64_MOVE_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    StoreLocal {
        name: "store_local", abi: TaggedWord, ops: ["StoreLocal"],
        x86: &X86_MOVE_BYTES, aarch64: &AARCH64_MOVE_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    TruthyPointer {
        name: "truthy_pointer_word", abi: ScalarWordBool, ops: ["JumpIfFalse"],
        x86: &X86_TRUTHY_POINTER_BYTES, aarch64: &AARCH64_TRUTHY_POINTER_BYTES,
        x86_holes: &[], aarch64_holes: &[]
    },
    LoadConst {
        name: "load_const", abi: ConstantWord, ops: ["LoadConst", "Return"],
        x86: &X86_LOAD_CONST_BYTES, aarch64: &AARCH64_LOAD_CONST_BYTES,
        x86_holes: &[(2, 8, "Literal64")],
        aarch64_holes: &[(8, 8, "Literal64")]
    },
    NullishWord {
        name: "nullish_word", abi: ScalarWordBool, ops: ["Unary", "Return"],
        x86: &X86_NULLISH_WORD_BYTES, aarch64: &AARCH64_NULLISH_WORD_BYTES,
        x86_holes: &[(9, 8, "Literal64")],
        aarch64_holes: &[(24, 8, "Literal64")]
    },
    TruthyWord {
        name: "truthy_word", abi: ScalarWordBool, ops: ["JumpIfFalse"],
        x86: &X86_TRUTHY_WORD_BYTES, aarch64: &AARCH64_TRUTHY_WORD_BYTES,
        x86_holes: &[(2, 8, "Literal64")],
        aarch64_holes: &[(16, 8, "Literal64")]
    },
}

pub(crate) fn rust_assembly_recipe(declaration: &RegionDeclaration) -> Option<RustAssemblyRecipe> {
    RUST_ASSEMBLY_DECLARATIONS
        .iter()
        .zip(RUST_ASSEMBLY_RECIPES)
        .find_map(|(candidate, recipe)| (candidate == declaration).then_some(*recipe))
}

#[cfg(test)]
pub(crate) fn rust_assembly_declaration(recipe: RustAssemblyRecipe) -> &'static RegionDeclaration {
    let index = RUST_ASSEMBLY_RECIPES
        .iter()
        .position(|candidate| *candidate == recipe)
        .expect("recipe belongs to canonical assembly catalog");
    &RUST_ASSEMBLY_DECLARATIONS[index]
}
