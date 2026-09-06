macro_rules! compare_branch_declaration {
    ($name:literal, $bytes:ident) => {
        RegionDeclaration {
            name: $name,
            operations: &["Binary", "JumpIfFalse"],
            abi: DeclAbi::CompareBranch,
            x86_bytes: &[0xC3],
            aarch64_bytes: &$bytes,
            portable_bytes: &[0xC3],
            holes: &[],
            aarch64_holes: &[],
            entry: 0,
            external_entries: &[0],
        }
    };
}

const COMPOSED_REGION_DECLARATIONS: &[RegionDeclaration] = &[
    RegionDeclaration {
        name: "dispatch",
        // Every compact opcode has an executable entry.  The entry is a
        // generated trampoline into the canonical Rust handler; it carries
        // no JavaScript semantics of its own and therefore remains valid for
        // operations whose specialized leaves are not yet available.
        operations: &[
            "LoadConst",
            "Move",
            "Add",
            "AddConst",
            "JumpIfFalse",
            "Return",
            "Slow",
            "LoadLocal",
            "Sub",
            "Mul",
            "Div",
            "GetProperty",
            "Call",
            "Jump",
            "IncI",
            "ForI",
            "AGetI",
            "ASetI",
            "AGetIInc",
            "GetN",
            "SetN",
            "CallN",
            "UpdateLocal",
            "LoadLocalChecked",
            "Binary",
            "StoreLocalChecked",
            "InitLocal",
            "StoreLocal",
            "GetPropertyQuickened",
            "GetNQuickened",
            "AGetIQuickened",
            "Unary",
        ],
        abi: DeclAbi::Bridge,
        // movabs rax, <bridge>; jmp rax. The context pointer remains the
        // platform ABI's first argument and is supplied for every invocation.
        x86_bytes: &X86_DISPATCH_BYTES,
        // ldr x16, #8; br x16; <bridge pointer>.  x0, the first ABI
        // argument, is left untouched for the canonical Rust bridge.
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "loop_glue",
        // This is the measured straight-line loop body from the neutral
        // arithmetic corpus.  The generated entry is a copy-and-patch bridge;
        // the bounded semantic executor validates and runs each operation.
        operations: &[
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Add",
            "StoreLocal",
            "Move",
        ],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "loop_body",
        // Profiled, branch-free loop body assembled from already-admitted
        // canonical handlers.  The sequential executor validates this full
        // window before invoking any handler, so a stale/unknown fact falls
        // back atomically to the ordinary interpreter.
        operations: &[
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Add",
            "StoreLocal",
            "Move",
            "UpdateLocal",
            "Return",
        ],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "binary_glue",
        operations: &["LoadLocal", "LoadConst", "Binary", "Return"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "update_return",
        operations: &["UpdateLocal", "Return"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "call",
        // Call remains semantically owned by the canonical call-IC handler;
        // this bounded leaf only removes the dispatch wrapper when its
        // callable fact is still valid.
        operations: &["Call"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "call_n",
        operations: &["CallN"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "arithmetic_glue",
        // Measured neutral arithmetic-loop glue. This bounded row remains a
        // build-time admission fact; execution uses the canonical handlers
        // until a physical implementation proves its boundary cost.
        operations: &[
            "LoadConst",
            "LoadLocalChecked",
            "Binary",
            "UpdateLocal",
            "StoreLocal",
        ],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "get_property",
        operations: &["GetProperty"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "set_named",
        operations: &["SetN"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "get_index",
        operations: &["AGetI"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_get_number",
        // ARM64 performs the proven dense numeric load from an explicit
        // element pointer; other targets retain the complete bridge.
        operations: &["AGetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_GET_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_set_number",
        // ARM64 performs the proven dense numeric store from an explicit
        // element pointer; other targets retain the complete bridge.
        operations: &["ASetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_SET_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_get_inc_number",
        // A proven dense numeric read plus induction update.  The index is
        // published as a scalar context field, never as a raw VM-word pointer.
        operations: &["AGetIInc"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_GET_INC_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_numeric_update",
        // The existing raw kernel composes indexed load, numeric add, and
        // indexed store while preserving the caller's register roles.
        operations: &["AGetI", "Add", "ASetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_KERNEL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_numeric_update_const",
        // The frontend commonly lowers a constant add as AddConst.  Keep its
        // pool operand in the canonical residual stream while reusing the
        // same physical load/add/store body.
        operations: &["AGetI", "AddConst", "ASetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_KERNEL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "set_index",
        operations: &["ASetI"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "get_index_inc",
        operations: &["AGetIInc"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "for_i",
        // Structured ForI has no bytecode back-edge, so this is a bounded
        // admission row only; the canonical loop handler remains complete.
        operations: &["ForI"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "add_chain",
        // Two proven numeric adds share one ABI entry and one return. The
        // runtime admits this row only when the second add consumes the first
        // result; all other shapes use canonical handlers.
        operations: &["Add", "Add"],
        abi: DeclAbi::ScalarF64x3,
        x86_bytes: &X86_ADD_CHAIN_BYTES,
        aarch64_bytes: &AARCH64_ADD_CHAIN_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_loop_body",
        // A bounded array-loop block. AArch64 uses a direct raw numeric
        // kernel; Rust performs the semantic admission and exact exit
        // materialization before/after this physical body.
        operations: &["LoadLocalChecked", "AGetI", "Add", "ASetI", "Return"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_KERNEL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    compare_branch_declaration!("compare_equal_branch", AARCH64_COMPARE_EQUAL_BRANCH_BYTES),
    compare_branch_declaration!(
        "compare_not_equal_branch",
        AARCH64_COMPARE_NOT_EQUAL_BRANCH_BYTES
    ),
    compare_branch_declaration!("compare_less_branch", AARCH64_COMPARE_LESS_BRANCH_BYTES),
    compare_branch_declaration!(
        "compare_less_equal_branch",
        AARCH64_COMPARE_LESS_EQUAL_BRANCH_BYTES
    ),
    compare_branch_declaration!("compare_greater_branch", AARCH64_COMPARE_GREATER_BRANCH_BYTES),
    compare_branch_declaration!(
        "compare_greater_equal_branch",
        AARCH64_COMPARE_GREATER_EQUAL_BRANCH_BYTES
    ),
    RegionDeclaration {
        name: "array_numeric_loop",
        operations: &[
            "LoadLocal",
            "LoadConst",
            "Binary",
            "JumpIfFalse",
            "LoadLocal",
            "Move",
            "LoadLocal",
            "Move",
            "LoadLocal",
            "Slow",
            "LoadLocal",
            "AGetI",
            "AddConst",
            "ASetI",
            "Move",
            "LoadLocal",
            "AddConst",
            "StoreLocal",
            "Jump",
        ],
        abi: DeclAbi::ArrayNumericLoop,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_LOOP_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
];
