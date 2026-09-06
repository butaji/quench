fn generate_stencil_catalog(declarations: &[RegionDeclaration]) {
    assert_unique_region_ids(declarations);
    declarations.iter().for_each(validate_catalog_declaration);
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let generated = render_catalog(declarations);
    fs::write(output.join("stencil_catalog.rs"), generated).expect("write stencil catalog");
    emit_catalog_rerun_inputs();
}

fn validate_catalog_declaration(declaration: &RegionDeclaration) {
    let byte_len = declaration
        .x86_bytes
        .len()
        .max(declaration.aarch64_bytes.len())
        .max(declaration.portable_bytes.len());
    validate_holes(declaration, byte_len, declaration.holes, "");
    validate_holes(
        declaration,
        declaration.aarch64_bytes.len(),
        declaration.aarch64_holes,
        "AArch64 ",
    );
    assert!(
        has_single_external_entry(declaration.entry, declaration.external_entries),
        "stencil {} has an external edge into its interior",
        declaration.name
    );
}

fn validate_holes(
    declaration: &RegionDeclaration,
    byte_len: usize,
    holes: &[(u16, usize, &'static str)],
    target: &str,
) {
    for (offset, width, _) in holes {
        assert!(
            usize::from(*offset) + *width <= byte_len,
            "stencil {} has an out-of-range {target}hole",
            declaration.name
        );
    }
}

struct CatalogParts {
    accessors: String,
    rows: String,
    bytes: String,
    holes: String,
    operations: String,
    keys: String,
    numeric_keys: String,
}

impl CatalogParts {
    fn derive(declarations: &[RegionDeclaration]) -> Self {
        Self {
            accessors: render_accessors(declarations),
            rows: render_region_rows(declarations),
            bytes: render_declarations(declarations, byte_decl),
            holes: render_declarations(declarations, hole_decl),
            operations: render_operations(declarations),
            keys: render_keys(declarations),
            numeric_keys: render_numeric_keys(declarations),
        }
    }
}

fn render_accessors(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(|declaration| {
            let accessor = accessor_name(declaration.name);
            let key = key_name(declaration.name);
            format!(
                "pub const fn {accessor}_region_id() -> crate::stencil_fact::RegionId {{ CANONICAL_{key}_ID }}\npub const fn {accessor}_region_key() -> crate::stencil_fact::RegionKey {{ CANONICAL_{key}_KEY }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_region_rows(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .enumerate()
        .map(|(index, declaration)| render_region_row(index, declaration))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_region_row(index: usize, declaration: &RegionDeclaration) -> String {
    let declaration_name = declaration.name;
    let name = key_name(declaration.name);
    let fallthrough = render_fallthrough(declaration);
    let executable = executable_expr(declaration);
    let abi = abi_expr(declaration);
    let external_entries = declaration
        .external_entries
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "    crate::stencil_select::RegionRecord {{ name: {declaration_name:?}, key: CANONICAL_{name}_KEY, stencil: crate::stencil_fact::Stencil {{ bytes: CANONICAL_{name}_BYTES, holes: CANONICAL_{name}_HOLES }}, operations: CANONICAL_{name}_OPS, entry: {entry}, external_entries: &[{external_entries}], fallthrough: {fallthrough}, abi: {abi}, template_calls_helper: {template_calls_helper}, executable: {executable} }}, // declaration {index}",
        entry = declaration.entry,
        template_calls_helper = target_template_calls_helper(declaration),
    )
}

fn render_fallthrough(declaration: &RegionDeclaration) -> String {
    let Some(recipe) = rust_assembly_recipe(declaration) else {
        return "None".to_owned();
    };
    let Some(continuation) = recipe.continuation() else {
        return "None".to_owned();
    };
    let tail = key_name(continuation.tail_name.trim_end_matches("_tail"));
    format!(
        "Some(crate::stencil_select::PhysicalFallthrough {{ stencil: &{tail}_TAIL, target: {:?} }})",
        continuation.target
    )
}

fn executable_expr(declaration: &RegionDeclaration) -> &'static str {
    match (declaration.name, declaration.abi) {
        ("dispatch", _) => "DISPATCH_EXECUTABLE",
        ("prototype_property", _) => "cfg!(target_arch = \"aarch64\")",
        (_, DeclAbi::PropertyGuard | DeclAbi::PropertyWriteGuard) => {
            "cfg!(any(target_arch = \"x86_64\", target_arch = \"aarch64\"))"
        }
        (_, DeclAbi::ArrayKernel) => "cfg!(target_arch = \"aarch64\")",
        (_, DeclAbi::CompareBranch) => "cfg!(target_arch = \"aarch64\")",
        _ => "EXECUTABLE",
    }
}

fn render_declarations(
    declarations: &[RegionDeclaration],
    render: fn(&str, &RegionDeclaration) -> String,
) -> String {
    declarations
        .iter()
        .map(|declaration| {
            render(
                &format!("CANONICAL_{}", key_name(declaration.name)),
                declaration,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_operations(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(|declaration| {
            let name = key_name(declaration.name);
            format!(
                "const CANONICAL_{name}_OPS: &[crate::ir::Opcode] = &[{}];",
                opcode_expr(declaration.operations)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_keys(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .map(|declaration| {
            let name = key_name(declaration.name);
            let id = stable_region_id(declaration.name);
            format!(
                "const CANONICAL_{name}_ID: crate::stencil_fact::RegionId = crate::stencil_fact::RegionId({id});\nconst CANONICAL_{name}_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(CANONICAL_{name}_ID, CANONICAL_{name}_OPS);"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_numeric_keys(declarations: &[RegionDeclaration]) -> String {
    declarations
        .iter()
        .filter(|declaration| is_numeric_scalar_leaf(declaration))
        .map(|declaration| {
            format!(
                "    (crate::ir::Opcode::{}, CANONICAL_{}_KEY),",
                declaration.operations[0],
                key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_catalog(declarations: &[RegionDeclaration]) -> String {
    let parts = CatalogParts::derive(declarations);
    let mut generated = String::from(
        r#"
// Generated from REGION_DECLARATIONS.  The declaration is the sole source of
// bytes, holes, operation contracts, keys, rows and accessors.
#[cfg(target_arch = "aarch64")]
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC0, 0x03, 0x5F, 0xD6];
#[cfg(not(target_arch = "aarch64"))]
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC3];
const FALLTHROUGH_TAIL_HOLES: &[crate::stencil_fact::Hole] = &[];
const FALLTHROUGH_TAIL: crate::stencil_fact::Stencil = crate::stencil_fact::Stencil {
    bytes: FALLTHROUGH_TAIL_BYTES,
    holes: FALLTHROUGH_TAIL_HOLES,
};
const EXECUTABLE: bool = cfg!(any(target_arch = "x86_64", target_arch = "aarch64"));
const DISPATCH_EXECUTABLE: bool = cfg!(target_arch = "x86_64");
"#,
    );
    generated.push_str(&composition_tail_declarations());
    generated.push('\n');
    generated.push_str(&generated_abi_catalog(declarations));
    generated.push('\n');
    generated.push_str(&parts.bytes);
    generated.push('\n');
    generated.push_str(&parts.holes);
    generated.push('\n');
    generated.push_str(&parts.operations);
    generated.push('\n');
    generated.push_str(&parts.keys);
    generated.push_str("\nstatic NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[\n");
    generated.push_str(&parts.numeric_keys);
    generated.push_str(
        r#"
];
static CANONICAL_REGION_TABLE: &[crate::stencil_select::RegionRecord] = &[
"#,
    );
    generated.push_str(&parts.rows);
    generated.push_str(
        r#"
];
fn canonical_region_lookup(key: crate::stencil_fact::RegionKey) -> Option<&'static crate::stencil_select::RegionRecord> {
    CANONICAL_REGION_TABLE.iter().find(|record| record.key == key)
}
"#,
    );
    generated.push_str(&parts.accessors);
    generated.push('\n');
    generated
}

fn emit_catalog_rerun_inputs() {
    println!("cargo:rerun-if-changed=build.rs");
    for input in [
        "catalog_render.rs",
        "catalog_validate.rs",
        "declarations_composed.rs",
        "declarations_leaf.rs",
        "declarations_tagged.rs",
        "driver.rs",
        "encoding_aarch64.rs",
        "encoding_common.rs",
        "encoding_verify.rs",
        "encoding_x86.rs",
    ] {
        println!("cargo:rerun-if-changed=build_stencil_catalog/{input}");
    }
    println!("cargo:rerun-if-changed=build_stencil_artifacts.rs");
    println!("cargo:rerun-if-changed=build_stencil_artifacts");
    println!("cargo:rerun-if-changed=build_stencil_contract.rs");
    println!("cargo:rerun-if-changed=build_stencil_templates.rs");
    println!("cargo:rerun-if-changed=src/ir.rs");
}

fn stable_region_id(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn assert_unique_region_ids(declarations: &[RegionDeclaration]) {
    let mut ids = std::collections::BTreeMap::new();
    for declaration in declarations {
        let id = stable_region_id(declaration.name);
        if let Some(previous) = ids.insert(id, declaration.name) {
            panic!("region ID collision: {previous} and {}", declaration.name);
        }
    }
}

fn is_numeric_scalar_leaf(declaration: &RegionDeclaration) -> bool {
    declaration.abi == DeclAbi::ScalarF64Binary
        && declaration.operations.last() == Some(&"Return")
        && declaration
            .operations
            .first()
            .is_some_and(|opcode| matches!(*opcode, "Add" | "Sub" | "Mul" | "Div" | "AddConst"))
}

/// Derive the helper boundary from the canonical ABI declaration.  A branch
/// instruction is not intrinsically a helper call; only bridge rows cross the
/// semantic Rust boundary, while raw/scalar rows remain leaf execution.
fn target_template_calls_helper(declaration: &RegionDeclaration) -> bool {
    matches!(declaration.abi, DeclAbi::Bridge)
}

fn has_single_external_entry(entry: u32, external_entries: &[u32]) -> bool {
    external_entries.len() == 1 && external_entries[0] == entry
}

fn accessor_name(name: &str) -> String {
    match name {
        "set_named" => "set_n".to_owned(),
        other => other.to_owned(),
    }
}

fn key_name(name: &str) -> String {
    match name {
        "set_named" => "SET_N".to_owned(),
        other => other.to_ascii_uppercase(),
    }
}

/// Derive the target view from the declaration's typed ABI family.  The bytes
/// are a physical consequence of this fact, never the source used to infer it.
fn abi_expr(declaration: &RegionDeclaration) -> &'static str {
    let target_is_aarch64 = env::var("CARGO_CFG_TARGET_ARCH")
        .ok()
        .is_some_and(|arch| arch == "aarch64")
        || env::var("TARGET")
            .ok()
            .is_some_and(|target| target.starts_with("aarch64"));
    match declaration.abi {
        DeclAbi::ScalarF64Binary => "crate::stencil_select::RegionAbi::ScalarF64Binary",
        DeclAbi::ScalarF64Unary => "crate::stencil_select::RegionAbi::ScalarF64Unary",
        DeclAbi::ScalarF64x3 => "crate::stencil_select::RegionAbi::ScalarF64x3",
        DeclAbi::TaggedWord => "crate::stencil_select::RegionAbi::TaggedWord",
        DeclAbi::ConstantWord => "crate::stencil_select::RegionAbi::ConstantWord",
        DeclAbi::ScalarBool => "crate::stencil_select::RegionAbi::ScalarBool",
        DeclAbi::ScalarWordBool => "crate::stencil_select::RegionAbi::ScalarWordBool",
        DeclAbi::ScalarWordPairBool => "crate::stencil_select::RegionAbi::ScalarWordPairBool",
        DeclAbi::ScalarI32 => "crate::stencil_select::RegionAbi::ScalarI32",
        DeclAbi::ScalarU32 => "crate::stencil_select::RegionAbi::ScalarU32",
        DeclAbi::PropertyGuard => "crate::stencil_select::RegionAbi::PropertyGuard",
        DeclAbi::PropertyWriteGuard => "crate::stencil_select::RegionAbi::PropertyWriteGuard",
        DeclAbi::Bridge => "crate::stencil_select::RegionAbi::Bridge",
        DeclAbi::ArrayKernel if target_is_aarch64 => {
            "crate::stencil_select::RegionAbi::ArrayKernel"
        }
        DeclAbi::ArrayNumericLoop if target_is_aarch64 => {
            "crate::stencil_select::RegionAbi::ArrayNumericLoop"
        }
        DeclAbi::CompareBranch if target_is_aarch64 => {
            "crate::stencil_select::RegionAbi::CompareBranch"
        }
        // The raw array ABI is only implemented on ARM64. Other targets keep
        // the same semantic declaration but route through the typed bridge.
        DeclAbi::ArrayKernel | DeclAbi::ArrayNumericLoop | DeclAbi::CompareBranch => {
            "crate::stencil_select::RegionAbi::Bridge"
        }
    }
}

/// Emit the Rust ABI catalog invocation from the same `DeclAbi` values that
/// drive every generated region row.  The selector macro owns the type-safe
/// contract shape; this build-time view owns only the mechanical field data.
fn generated_abi_catalog(declarations: &[RegionDeclaration]) -> String {
    let mut variants = Vec::new();
    for declaration in declarations {
        if !variants.contains(&declaration.abi) {
            variants.push(declaration.abi);
        }
    }
    let rows = variants
        .into_iter()
        .map(|abi| {
            let (name, context, fields) = abi_contract_fields(abi);
            format!("    {name} => {{ context: {context}, {fields} }}")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("region_abi_catalog! {{\n{rows},\n}}")
}

fn abi_contract_fields(abi: DeclAbi) -> (&'static str, bool, &'static str) {
    match abi {
        DeclAbi::ScalarF64Binary
        | DeclAbi::ScalarF64Unary
        | DeclAbi::ScalarF64x3
        | DeclAbi::TaggedWord
        | DeclAbi::ConstantWord
        | DeclAbi::ScalarBool
        | DeclAbi::ScalarWordBool
        | DeclAbi::ScalarWordPairBool
        | DeclAbi::ScalarI32
        | DeclAbi::ScalarU32 => (
            abi_variant_name(abi),
            false,
            "context_words: 0, preserves_vm_registers: true, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0, hardware_gpr_clobber_mask: 0, live_out_mask: 1, root_materialization_required: false",
        ),
        DeclAbi::Bridge => (
            "Bridge",
            true,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: true, interruptible_backedge: false, hardware_clobber_mask: 0xffff, hardware_gpr_clobber_mask: 0xffff, live_out_mask: 0xffff, root_materialization_required: true",
        ),
        DeclAbi::ArrayKernel => (
            "ArrayKernel",
            true,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0x0003, hardware_gpr_clobber_mask: 0x001f, live_out_mask: 1, root_materialization_required: false",
        ),
        DeclAbi::ArrayNumericLoop => (
            "ArrayNumericLoop",
            true,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: true, hardware_clobber_mask: 0x0007, hardware_gpr_clobber_mask: 0x007f, live_out_mask: 0x0003, root_materialization_required: false",
        ),
        DeclAbi::CompareBranch => (
            "CompareBranch",
            true,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0x0003, hardware_gpr_clobber_mask: 0x0007, live_out_mask: 0x0003, root_materialization_required: false",
        ),
        DeclAbi::PropertyGuard => (
            "PropertyGuard",
            true,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0, hardware_gpr_clobber_mask: 0x000f, live_out_mask: 1, root_materialization_required: false",
        ),
        DeclAbi::PropertyWriteGuard => (
            "PropertyWriteGuard",
            true,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0, hardware_gpr_clobber_mask: 0x000f, live_out_mask: 0, root_materialization_required: false",
        ),
    }
}

fn abi_variant_name(abi: DeclAbi) -> &'static str {
    match abi {
        DeclAbi::ScalarF64Binary => "ScalarF64Binary",
        DeclAbi::ScalarF64Unary => "ScalarF64Unary",
        DeclAbi::ScalarF64x3 => "ScalarF64x3",
        DeclAbi::TaggedWord => "TaggedWord",
        DeclAbi::ConstantWord => "ConstantWord",
        DeclAbi::ScalarBool => "ScalarBool",
        DeclAbi::ScalarWordBool => "ScalarWordBool",
        DeclAbi::ScalarWordPairBool => "ScalarWordPairBool",
        DeclAbi::ScalarI32 => "ScalarI32",
        DeclAbi::ScalarU32 => "ScalarU32",
        DeclAbi::Bridge => "Bridge",
        DeclAbi::ArrayKernel => "ArrayKernel",
        DeclAbi::ArrayNumericLoop => "ArrayNumericLoop",
        DeclAbi::CompareBranch => "CompareBranch",
        DeclAbi::PropertyGuard => "PropertyGuard",
        DeclAbi::PropertyWriteGuard => "PropertyWriteGuard",
    }
}

fn opcode_expr(operations: &[&str]) -> String {
    operations
        .iter()
        .map(|name| format!("crate::ir::Opcode::{name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn hole_decl(name: &str, declaration: &RegionDeclaration) -> String {
    let holes = declaration
        .holes
        .iter()
        .map(|(offset, _, kind)| {
            format!(
                "crate::stencil_fact::Hole {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let arm_holes = declaration
        .aarch64_holes
        .iter()
        .map(|(offset, _, kind)| {
            format!(
                "crate::stencil_fact::Hole {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[{holes}];\n#[cfg(target_arch = \"aarch64\")]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[{arm_holes}];\n#[cfg(not(any(target_arch = \"x86_64\", target_arch = \"aarch64\")))]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[];"
    )
}

fn byte_decl(name: &str, declaration: &RegionDeclaration) -> String {
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst {name}_BYTES: &[u8] = &[{}];\n#[cfg(target_arch = \"aarch64\")]\nconst {name}_BYTES: &[u8] = &[{}];\n#[cfg(not(any(target_arch = \"x86_64\", target_arch = \"aarch64\")))]\nconst {name}_BYTES: &[u8] = &[{}];",
        bytes_expr(declaration.x86_bytes),
        bytes_expr(declaration.aarch64_bytes),
        bytes_expr(declaration.portable_bytes),
    )
}

fn composition_tail_declarations() -> String {
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst ADD_CHAIN_TAIL_BYTES: &[u8] = &[{}];\n#[cfg(target_arch = \"aarch64\")]\nconst ADD_CHAIN_TAIL_BYTES: &[u8] = &[{}];\n#[cfg(not(any(target_arch = \"x86_64\", target_arch = \"aarch64\")))]\nconst ADD_CHAIN_TAIL_BYTES: &[u8] = &[];\nconst ADD_CHAIN_TAIL: crate::stencil_fact::Stencil = crate::stencil_fact::Stencil {{ bytes: ADD_CHAIN_TAIL_BYTES, holes: &[] }};",
        bytes_expr(&X86_ADD_CHAIN_TAIL_BYTES),
        bytes_expr(&AARCH64_ADD_CHAIN_TAIL_BYTES),
    )
}

fn bytes_expr(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("0x{byte:02X}"))
        .collect::<Vec<_>>()
        .join(", ")
}
