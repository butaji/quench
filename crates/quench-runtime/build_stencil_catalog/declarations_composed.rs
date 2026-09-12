// A bridge has one physical contract regardless of its semantic window:
// tail-call the canonical Rust handler with the context pointer unchanged.
macro_rules! bridge_region {
    ($name:literal, $operations:expr $(,)?) => {
        RegionDeclaration {
            name: $name,
            operations: $operations,
            abi: DeclAbi::Bridge,
            // movabs rax, <bridge>; jmp rax.
            x86_bytes: &X86_DISPATCH_BYTES,
            // ldr x16, #8; br x16; <bridge pointer>.
            aarch64_bytes: &AARCH64_DISPATCH_BYTES,
            portable_bytes: &[0xC3],
            holes: &[(2, 8, "Ptr64")],
            aarch64_holes: &[(8, 8, "Ptr64")],
            entry: 0,
            external_entries: &[0],
        }
    };
}

const COMPOSED_REGION_DECLARATIONS: &[RegionDeclaration] = &[
    RegionDeclaration {
        name: "dense_numeric_copy_loop",
        operations: &[
            "LoadConst",
            "LoadConst",
            "StoreLocal",
            "LoadConst",
            "LoadLocal",
            "LoadLocal",
            "GetN",
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
            "ASetI",
            "Move",
            "LoadLocal",
            "LoadConst",
            "Binary",
            "StoreLocal",
            "Unary",
            "Jump",
            "LoadLocal",
            "Slow",
            "LoadConst",
            "AGetI",
            "LoadLocal",
            "Slow",
            "LoadLocal",
            "GetN",
            "LoadConst",
            "Sub",
            "AGetI",
            "Add",
            "Return",
        ],
        abi: DeclAbi::ArrayCopyLoop,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_COPY_LOOP_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "ordered_f64_reduction_loop",
        operations: &[
            "LoadConst",
            "StoreLocal",
            "LoadConst",
            "LoadConst",
            "StoreLocal",
            "LoadConst",
            "LoadLocal",
            "LoadLocal",
            "GetN",
            "Binary",
            "JumpIfFalse",
            "LoadLocal",
            "LoadLocal",
            "Slow",
            "LoadLocal",
            "AGetI",
            "Add",
            "StoreLocal",
            "Move",
            "LoadLocal",
            "LoadConst",
            "Binary",
            "StoreLocal",
            "Unary",
            "Jump",
            "LoadLocal",
            "Return",
        ],
        abi: DeclAbi::ArrayReductionLoop,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_REDUCTION_LOOP_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "conditional_f64_reduction_loop",
        operations: &[
            "LoadConst",
            "StoreLocal",
            "LoadConst",
            "LoadConst",
            "StoreLocal",
            "LoadConst",
            "LoadLocal",
            "LoadLocal",
            "GetN",
            "Binary",
            "JumpIfFalse",
            "LoadLocal",
            "LoadLocal",
            "GetN",
            "Slow",
            "LoadLocal",
            "AGetI",
            "LoadConst",
            "Binary",
            "JumpIfFalse",
            "LoadConst",
            "Move",
            "Jump",
            "LoadConst",
            "Unary",
            "Move",
            "Add",
            "StoreLocal",
            "Move",
            "LoadLocal",
            "LoadConst",
            "Binary",
            "StoreLocal",
            "Unary",
            "Jump",
            "LoadLocal",
            "Return",
            "LoadConst",
            "Return",
        ],
        abi: DeclAbi::ArrayReductionLoop,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_CONDITIONAL_REDUCTION_LOOP_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    bridge_region!(
        "dispatch",
        // Every compact opcode has an executable entry.  The entry is a
        // generated trampoline into the canonical Rust handler; it carries
        // no JavaScript semantics of its own and therefore remains valid for
        // operations whose specialized leaves are not yet available.
        &[
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
            "Remainder",
            "Exponentiate",
            "MarkUninitialized",
            "MarkImmutable",
            "RequireObjectCoercible",
            "NumericAdd",
            "NumericSubtract",
            "Equal",
            "NotEqual",
            "StrictEqual",
            "StrictNotEqual",
            "LessThan",
            "LessEqual",
            "GreaterThan",
            "GreaterEqual",
            "BitwiseOr",
            "BitwiseXor",
            "BitwiseAnd",
            "ShiftLeft",
            "ShiftRight",
            "ShiftRightZeroFill",
            "Instanceof",
            "Loop",
            "TailCall",
            "MakeArray",
            "MakeFunctionWithKind",
            "SetFunctionName",
            "MakeObject",
            "Construct",
            "ForOf",
            "CallSlow",
            "Try",
            "Await",
            "MakeBuiltin",
            "ValidateClassHeritage",
            "GetClassPrototype",
            "MakeFunction",
            "StaticBlock",
            "AppendInstanceField",
            "PrivateScope",
            "LoadParameter",
            "InitializeLocal",
            "CheckInitialized",
            "Throw",
        ]
    ),
    bridge_region!(
        "loop_glue",
        // This is the measured straight-line loop body from the neutral
        // arithmetic corpus.  The generated entry is a copy-and-patch bridge;
        // the bounded semantic executor validates and runs each operation.
        &[
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Add",
            "StoreLocal",
            "Move",
        ]
    ),
    bridge_region!(
        "loop_body",
        // Profiled, branch-free loop body assembled from already-admitted
        // canonical handlers.  The sequential executor validates this full
        // window before invoking any handler, so a stale/unknown fact falls
        // back atomically to the ordinary interpreter.
        &[
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Add",
            "StoreLocal",
            "Move",
            "UpdateLocal",
            "Return",
        ]
    ),
    bridge_region!(
        "binary_glue",
        &["LoadLocal", "LoadConst", "Binary", "Return"]
    ),
    bridge_region!(
        "binary_branch_glue",
        // A minimal CFG witness: the generated bridge enters once, while the
        // canonical region executor follows the verified forward join and
        // consumes the guarded scalar binary interior.
        &[
            "LoadLocal",
            "LoadLocal",
            "Binary",
            "JumpIfFalse",
            "LoadConst",
            "Move",
            "Jump",
            "LoadConst",
            "Move",
            "Return",
        ]
    ),
    bridge_region!(
        "nested_branch_glue",
        // Two nested conditional blocks converge through one verified join;
        // each arm remains canonical after the physical control transfer.
        &[
            "LoadLocal",
            "JumpIfFalse",
            "LoadLocal",
            "JumpIfFalse",
            "LoadConst",
            "Jump",
            "LoadConst",
            "Jump",
            "LoadConst",
            "Return",
        ]
    ),
    bridge_region!(
        "branch_glue",
        // Control-only CFG witness. The physical branch leaf is guarded by a
        // Boolean proof; other values stay on this verified canonical path.
        &["JumpIfFalse", "Move", "Jump", "Move", "Return"]
    ),
    bridge_region!(
        "store_glue",
        // A minimal environment boundary witness: a proven tagged-word load
        // feeds a proven direct local store before the canonical return.
        &["LoadLocal", "StoreLocal", "Return"]
    ),
    bridge_region!(
        "checked_store_glue",
        // Checked local pairs retain the canonical initialized/deleted guard;
        // only the proven direct word transfer enters the physical leaf.
        &["LoadLocalChecked", "StoreLocalChecked", "Return"]
    ),
    bridge_region!(
        "parameter_glue",
        // Parameter reads use the same proven tagged-word load as ordinary
        // locals; the environment pointer remains the guard authority.
        &["LoadParameter", "Return"]
    ),
    bridge_region!(
        "counted_glue",
        // A small CFG loop witness. Constants, comparison, transfers, and
        // the +1 register update are physical leaves; Return remains canonical.
        &[
            "LoadConst",
            "LoadConst",
            "LessThan",
            "JumpIfFalse",
            "IncI",
            "Jump",
            "Return",
        ]
    ),
    bridge_region!(
        "counted_decrement_glue",
        // Downward counted loops use the distinct -1 IncI artifact while
        // retaining the same verified CFG/backedge and interruption boundary.
        &[
            "LoadConst",
            "LoadConst",
            "GreaterThan",
            "JumpIfFalse",
            "IncI",
            "Jump",
            "Return",
        ]
    ),
    bridge_region!(
        "counted_continue_glue",
        // A counted loop with nested skip/break conditions.  The shared CFG
        // owns both forward exits and the resident continue backedge; leaves
        // consume only proven scalar/word operations on each visited path.
        &[
            "LoadConst",
            "LoadConst",
            "Binary",
            "JumpIfFalse",
            "LoadLocal",
            "JumpIfFalse",
            "LoadLocal",
            "JumpIfFalse",
            "Jump",
            "IncI",
            "Jump",
            "Jump",
            "Return",
        ]
    ),
    bridge_region!(
        "inc_glue",
        // The generated increment leaf consumes a proven numeric register;
        // the canonical return remains the semantic boundary.
        &["IncI", "Return"]
    ),
    bridge_region!(
        "unary_glue",
        // Numeric unary leaves retain exact Number/ToInt32 semantics; the
        // canonical return remains the semantic boundary.
        &["Unary", "Return"]
    ),
    bridge_region!("update_return", &["UpdateLocal", "Return"]),
    bridge_region!(
        "call",
        // Call remains semantically owned by the canonical call-IC handler;
        // this bounded leaf only removes the dispatch wrapper when its
        // callable fact is still valid.
        &["Call"]
    ),
    bridge_region!("call_n", &["CallN"]),
    bridge_region!(
        "arithmetic_glue",
        // Measured neutral arithmetic-loop glue. This bounded row remains a
        // build-time admission fact; execution uses the canonical handlers
        // until a physical implementation proves its boundary cost.
        &[
            "LoadConst",
            "LoadLocalChecked",
            "Binary",
            "UpdateLocal",
            "StoreLocal",
        ]
    ),
    bridge_region!("get_property", &["GetProperty"]),
    bridge_region!("set_named", &["SetN"]),
    bridge_region!("get_index", &["AGetI"]),
    bridge_region!("set_index", &["ASetI"]),
    bridge_region!("get_index_inc", &["AGetIInc"]),
    bridge_region!(
        "for_i",
        // Structured ForI has no bytecode back-edge, so this is a bounded
        // admission row only; the canonical loop handler remains complete.
        &["ForI"]
    ),
];
