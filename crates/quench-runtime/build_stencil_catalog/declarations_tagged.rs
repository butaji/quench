const TAGGED_REGION_DECLARATIONS: &[RegionDeclaration] = &[
    RegionDeclaration {
        name: "load_const",
        operations: &["LoadConst", "Return"],
        abi: DeclAbi::ConstantWord,
        x86_bytes: &X86_LOAD_CONST_BYTES,
        aarch64_bytes: &AARCH64_LOAD_CONST_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Literal64")],
        aarch64_holes: &[(8, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "load_local",
        // A proven, non-cell lexical slot is the same physical tagged-word
        // load as Move, but has a distinct declaration so opcode/ABI routing
        // cannot infer compatibility from bytes alone.
        operations: &["LoadLocal"],
        abi: DeclAbi::TaggedWord,
        x86_bytes: &X86_MOVE_BYTES,
        aarch64_bytes: &AARCH64_MOVE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "store_local",
        // The physical entry reads the source register word; the canonical
        // ownership-aware commit happens after the typed leaf returns.
        operations: &["StoreLocal"],
        abi: DeclAbi::TaggedWord,
        x86_bytes: &X86_MOVE_BYTES,
        aarch64_bytes: &AARCH64_MOVE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "store_property",
        // All potentially failing semantic checks precede the single native
        // word store. Admission restricts both words to non-owning tags.
        operations: &["SetN"],
        abi: DeclAbi::PropertyWriteGuard,
        x86_bytes: &X86_PROPERTY_WRITE_BYTES,
        aarch64_bytes: &AARCH64_PROPERTY_WRITE_GUARD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "nullish_word",
        operations: &["Unary", "Return"],
        abi: DeclAbi::ScalarWordBool,
        x86_bytes: &X86_NULLISH_WORD_BYTES,
        aarch64_bytes: &AARCH64_NULLISH_WORD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(9, 8, "Literal64")],
        aarch64_holes: &[(24, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "truthy_word",
        // The caller guards this entry to canonical Bool/Null/Undefined
        // words.  Only the true Bool payload is truthy; all other admitted
        // payloads are false.  Number and heap words use ordinary semantics.
        operations: &["JumpIfFalse"],
        abi: DeclAbi::ScalarWordBool,
        x86_bytes: &X86_TRUTHY_WORD_BYTES,
        aarch64_bytes: &AARCH64_TRUTHY_WORD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Literal64")],
        aarch64_holes: &[(16, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "truthy_pointer_word",
        // Object/array/function pointer tags are always truthy. Strings and
        // other heap payloads remain on the complete coercion path because
        // their truthiness depends on observable contents.
        operations: &["JumpIfFalse"],
        abi: DeclAbi::ScalarWordBool,
        x86_bytes: &X86_TRUTHY_POINTER_BYTES,
        aarch64_bytes: &AARCH64_TRUTHY_POINTER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
];

const REGION_DECLARATION_GROUPS: &[&[RegionDeclaration]] = &[
    RUST_LEAF_DECLARATIONS,
    LEAF_ASSEMBLY_DECLARATIONS,
    COMPOSED_REGION_DECLARATIONS,
    TAGGED_REGION_DECLARATIONS,
];

fn region_declarations() -> Vec<RegionDeclaration> {
    REGION_DECLARATION_GROUPS
        .iter()
        .flat_map(|group| group.iter().cloned())
        .collect()
}
