use std::{env, fs, path::PathBuf};

#[derive(Clone, Copy)]
struct RegionDeclaration {
    name: &'static str,
    operations: &'static [&'static str],
    x86_bytes: &'static [u8],
    portable_bytes: &'static [u8],
    holes: &'static [(u16, usize, &'static str)],
    entry: u32,
    external_entries: &'static [u32],
}

const REGION_DECLARATIONS: &[RegionDeclaration] = &[
    RegionDeclaration {
        name: "loop",
        operations: &["Add", "Return"],
        x86_bytes: &[0xF2, 0x0F, 0x58, 0xC1, 0xC3],
        portable_bytes: &[0xC3],
        holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "property",
        operations: &["GetN"],
        // The property leaf only loads a word from a slot that the complete
        // shape/accessor validator has already proven.  Ownership is retained
        // by the Rust register writer after the leaf returns the raw word.
        x86_bytes: &[0x48, 0x8B, 0x07, 0xC3],
        portable_bytes: &[0xC3],
        holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "move",
        operations: &["Move"],
        // A pure Move leaf copies one canonical tagged word.  RegisterFile
        // performs the retain/release edge after this raw load returns.
        x86_bytes: &[0x48, 0x8B, 0x07, 0xC3],
        portable_bytes: &[0xC3],
        holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "fallthrough",
        operations: &["Add", "Return"],
        x86_bytes: &[0xF2, 0x0F, 0x58, 0xC1, 0xE9, 0, 0, 0, 0],
        portable_bytes: &[0xC3],
        holes: &[(5, 4, "Rel32")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "subtract",
        operations: &["Sub", "Return"],
        x86_bytes: &[0xF2, 0x0F, 0x5C, 0xC1, 0xC3],
        portable_bytes: &[0xC3],
        holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "multiply",
        operations: &["Mul", "Return"],
        x86_bytes: &[0xF2, 0x0F, 0x59, 0xC1, 0xC3],
        portable_bytes: &[0xC3],
        holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "divide",
        operations: &["Div", "Return"],
        x86_bytes: &[0xF2, 0x0F, 0x5E, 0xC1, 0xC3],
        portable_bytes: &[0xC3],
        holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "add_const",
        operations: &["AddConst", "Return"],
        x86_bytes: &[
            0xF2, 0x0F, 0x10, 0x0D, 0x05, 0x00, 0x00, 0x00, 0xF2, 0x0F, 0x58, 0xC1, 0xC3, 0, 0, 0,
            0, 0, 0, 0, 0,
        ],
        portable_bytes: &[0xC3],
        holes: &[(13, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
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
        ],
        // movabs rax, <bridge>; jmp rax. The context pointer remains the
        // platform ABI's first argument and is supplied for every invocation.
        x86_bytes: &[0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xE0],
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
];

fn main() {
    generate_op_names();
    generate_stencil_catalog();
    validate_stencil_declarations();
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rerun-if-env-changed=PROFILE");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    // Keep this mapping exhaustive: a profile not represented here must not
    // silently masquerade as a production artifact.
    let lto = match profile.as_str() {
        "production" | "release" => "fat",
        "release-thin" => "thin",
        "debug" | "unknown" => "off",
        other => panic!("unsupported Cargo profile for quench runtime: {other}"),
    };
    let production = matches!(profile.as_str(), "release" | "production" | "release-thin");
    if production {
        println!("cargo:rustc-cfg=quench_production");
    }
    println!("cargo:rustc-env=QUENCH_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=QUENCH_BUILD_LTO={lto}");
}

fn generate_stencil_catalog() {
    // The catalog is intentionally small and declarative here; these checks
    // make an invalid closed-hole relocation fail the build, rather than
    // reaching the runtime patcher.
    for declaration in REGION_DECLARATIONS {
        // Validate the largest emitted form here; target-specific generated
        // hole tables below remove the x86-only relocation on other ISAs.
        let byte_len = declaration
            .x86_bytes
            .len()
            .max(declaration.portable_bytes.len());
        for (offset, width, _) in declaration.holes {
            assert!(
                usize::from(*offset) + *width <= byte_len,
                "stencil {} has an out-of-range hole",
                declaration.name
            );
        }
        assert!(
            has_single_external_entry(declaration.entry, declaration.external_entries),
            "stencil {} has an external edge into its interior",
            declaration.name
        );
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let generated = r#"
// x86-64 SysV: `addsd xmm0,xmm1; ret`.  This is the build-time stencil for the
// proven-number Add+Return region.  Non-x86 targets receive a return-only
// fragment and must use the ordinary fallback entry.
__LOOP_BYTES__
// Property access is admitted only after the Rust shape/accessor validator;
// the leaf loads a tagged slot word and returns ownership to the Rust writer.
__PROPERTY_BYTES__
// Pure Move copies one canonical tagged word; the Rust register writer owns
// the retain/release edge after the leaf returns.
__MOVE_BYTES__
__FALLTHROUGH_BYTES__
__SUBTRACT_BYTES__
__MULTIPLY_BYTES__
__DIVIDE_BYTES__
__ADD_CONST_BYTES__
__DISPATCH_BYTES__
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC3];
// The catalog remains present on every target for deterministic admission,
// but only the ISA whose bytes are actually defined may cross the executable
// boundary. Unsupported targets must take the complete Rust fallback.
const X86_EXECUTABLE: bool = cfg!(target_arch = "x86_64");
__LOOP_HOLES__
__PROPERTY_HOLES__
__MOVE_HOLES__
__FALLTHROUGH_HOLES__
__SUBTRACT_HOLES__
__MULTIPLY_HOLES__
__DIVIDE_HOLES__
__ADD_CONST_HOLES__
__DISPATCH_HOLES__
const FALLTHROUGH_TAIL_HOLES: &[crate::stencil_fact::Hole] = &[];
const FALLTHROUGH_TAIL: crate::stencil_fact::Stencil = crate::stencil_fact::Stencil {
    bytes: FALLTHROUGH_TAIL_BYTES,
    holes: FALLTHROUGH_TAIL_HOLES,
};
const SUBTRACT_OPS: &[crate::ir::Opcode] = &[__SUBTRACT_OPS__];
const MULTIPLY_OPS: &[crate::ir::Opcode] = &[__MULTIPLY_OPS__];
const DIVIDE_OPS: &[crate::ir::Opcode] = &[__DIVIDE_OPS__];
const ADD_CONST_OPS: &[crate::ir::Opcode] = &[__ADD_CONST_OPS__];
const LOOP_OPS: &[crate::ir::Opcode] = &[__LOOP_OPS__];
const PROPERTY_OPS: &[crate::ir::Opcode] = &[__PROPERTY_OPS__];
const MOVE_OPS: &[crate::ir::Opcode] = &[__MOVE_OPS__];
const DISPATCH_OPS: &[crate::ir::Opcode] = crate::ir::Opcode::ALL;
const _: () = assert!(DISPATCH_OPS.len() == crate::ir::Opcode::COUNT as usize);
const FALLTHROUGH_OPS: &[crate::ir::Opcode] = &[__FALLTHROUGH_OPS__];
const LOOP_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(1), LOOP_OPS,
);
const PROPERTY_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(2), PROPERTY_OPS,
);
const MOVE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(8), MOVE_OPS,
);
const FALLTHROUGH_KEY: crate::stencil_fact::RegionKey =
    crate::stencil_fact::RegionKey::from_opcodes(crate::stencil_fact::RegionId(3), FALLTHROUGH_OPS);
const SUBTRACT_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(4), SUBTRACT_OPS,
);
const MULTIPLY_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(5), MULTIPLY_OPS,
);
const DIVIDE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(6), DIVIDE_OPS,
);
const ADD_CONST_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(7), ADD_CONST_OPS,
);
const DISPATCH_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(9), DISPATCH_OPS,
);
static NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[
    (crate::ir::Opcode::Add, FALLTHROUGH_KEY),
    (crate::ir::Opcode::Sub, SUBTRACT_KEY),
    (crate::ir::Opcode::Mul, MULTIPLY_KEY),
    (crate::ir::Opcode::Div, DIVIDE_KEY),
    (crate::ir::Opcode::AddConst, ADD_CONST_KEY),
];
static REGION_TABLE: &[crate::stencil_select::RegionRecord] = &[
    (crate::stencil_select::RegionRecord {
        key: LOOP_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: LOOP_BYTES, holes: LOOP_HOLES },
        operations: LOOP_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: MOVE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: MOVE_BYTES, holes: MOVE_HOLES },
        operations: MOVE_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: PROPERTY_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: PROPERTY_BYTES, holes: PROPERTY_HOLES },
        operations: PROPERTY_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: FALLTHROUGH_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: FALLTHROUGH_BYTES, holes: FALLTHROUGH_HOLES },
        operations: FALLTHROUGH_OPS,
        entry: 0,
        fallthrough: Some((&FALLTHROUGH_TAIL, 5)),
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: SUBTRACT_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: SUBTRACT_BYTES, holes: SUBTRACT_HOLES },
        operations: SUBTRACT_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: MULTIPLY_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: MULTIPLY_BYTES, holes: MULTIPLY_HOLES },
        operations: MULTIPLY_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: DIVIDE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: DIVIDE_BYTES, holes: DIVIDE_HOLES },
        operations: DIVIDE_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: ADD_CONST_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: ADD_CONST_BYTES, holes: ADD_CONST_HOLES },
        operations: ADD_CONST_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: DISPATCH_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: DISPATCH_BYTES, holes: DISPATCH_HOLES },
        operations: DISPATCH_OPS,
        entry: 0,
        fallthrough: None,
        executable: X86_EXECUTABLE,
    }),
];
"#
    .replace("__LOOP_BYTES__", &byte_decl("LOOP", &REGION_DECLARATIONS[0]))
    .replace("__PROPERTY_BYTES__", &byte_decl("PROPERTY", &REGION_DECLARATIONS[1]))
    .replace("__MOVE_BYTES__", &byte_decl("MOVE", &REGION_DECLARATIONS[2]))
    .replace(
        "__FALLTHROUGH_BYTES__",
        &byte_decl("FALLTHROUGH", &REGION_DECLARATIONS[3]),
    )
    .replace("__SUBTRACT_BYTES__", &byte_decl("SUBTRACT", &REGION_DECLARATIONS[4]))
    .replace("__MULTIPLY_BYTES__", &byte_decl("MULTIPLY", &REGION_DECLARATIONS[5]))
    .replace("__DIVIDE_BYTES__", &byte_decl("DIVIDE", &REGION_DECLARATIONS[6]))
    .replace(
        "__ADD_CONST_BYTES__",
        &byte_decl("ADD_CONST", &REGION_DECLARATIONS[7]),
    )
    .replace("__DISPATCH_BYTES__", &byte_decl("DISPATCH", &REGION_DECLARATIONS[8]))
    .replace("__LOOP_HOLES__", &hole_decl("LOOP", &REGION_DECLARATIONS[0]))
    .replace("__PROPERTY_HOLES__", &hole_decl("PROPERTY", &REGION_DECLARATIONS[1]))
    .replace("__MOVE_HOLES__", &hole_decl("MOVE", &REGION_DECLARATIONS[2]))
    .replace(
        "__FALLTHROUGH_HOLES__",
        &hole_decl("FALLTHROUGH", &REGION_DECLARATIONS[3]),
    )
    .replace("__SUBTRACT_HOLES__", &hole_decl("SUBTRACT", &REGION_DECLARATIONS[4]))
    .replace("__MULTIPLY_HOLES__", &hole_decl("MULTIPLY", &REGION_DECLARATIONS[5]))
    .replace("__DIVIDE_HOLES__", &hole_decl("DIVIDE", &REGION_DECLARATIONS[6]))
    .replace(
        "__ADD_CONST_HOLES__",
        &hole_decl("ADD_CONST", &REGION_DECLARATIONS[7]),
    )
    .replace("__DISPATCH_HOLES__", &hole_decl("DISPATCH", &REGION_DECLARATIONS[8]))
    .replace("__LOOP_OPS__", &opcode_expr(REGION_DECLARATIONS[0].operations))
    .replace("__PROPERTY_OPS__", &opcode_expr(REGION_DECLARATIONS[1].operations))
    .replace("__MOVE_OPS__", &opcode_expr(REGION_DECLARATIONS[2].operations))
    .replace("__FALLTHROUGH_OPS__", &opcode_expr(REGION_DECLARATIONS[3].operations))
    .replace("__SUBTRACT_OPS__", &opcode_expr(REGION_DECLARATIONS[4].operations))
    .replace("__MULTIPLY_OPS__", &opcode_expr(REGION_DECLARATIONS[5].operations))
    .replace("__DIVIDE_OPS__", &opcode_expr(REGION_DECLARATIONS[6].operations))
    .replace("__ADD_CONST_OPS__", &opcode_expr(REGION_DECLARATIONS[7].operations));
    fs::write(output.join("stencil_catalog.rs"), generated).expect("write stencil catalog");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/ir.rs");
}

fn has_single_external_entry(entry: u32, external_entries: &[u32]) -> bool {
    external_entries.len() == 1 && external_entries[0] == entry
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
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[{holes}];\n#[cfg(not(target_arch = \"x86_64\"))]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[];"
    )
}

fn byte_decl(name: &str, declaration: &RegionDeclaration) -> String {
    fn bytes_expr(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| format!("0x{byte:02X}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst {name}_BYTES: &[u8] = &[{}];\n#[cfg(not(target_arch = \"x86_64\"))]\nconst {name}_BYTES: &[u8] = &[{}];",
        bytes_expr(declaration.x86_bytes),
        bytes_expr(declaration.portable_bytes),
    )
}

/// Keep the build-time stencil catalog tied to the canonical data modules.
/// The generated bytes remain data; executable-memory effects are confined to
/// the runtime arena.
fn validate_stencil_declarations() {
    let facts = fs::read_to_string("src/stencil_fact.rs").expect("read stencil facts");
    let selector = fs::read_to_string("src/stencil_select.rs").expect("read stencil selector");
    let ir = fs::read_to_string("src/ir.rs").expect("read canonical opcode declaration");
    for required in ["RegionKey", "HoleKind", "PatchValues", "BoxingFact"] {
        assert!(facts.contains(required), "stencil facts missing {required}");
    }
    for required in [
        "select_stencil",
        "has_single_entry_point",
        "reduce_type_checks",
    ] {
        assert!(
            selector.contains(required),
            "stencil selector missing {required}"
        );
    }
    assert!(
        selector.contains("promotion_admitted"),
        "stencil promotion predicate missing"
    );
    for declaration in REGION_DECLARATIONS {
        for operation in declaration.operations {
            assert!(
                ir.contains(&format!("{operation} =")),
                "region {} names unknown opcode {operation}",
                declaration.name
            );
        }
    }
    println!("cargo:rerun-if-changed=src/stencil_fact.rs");
    println!("cargo:rerun-if-changed=src/stencil_select.rs");
}

fn generate_op_names() {
    let source = fs::read_to_string("src/ops_op.rs").expect("read canonical Op declaration");
    let variants = extract_op_variants(&source);
    assert!(variants.len() >= 90, "incomplete Op variant extraction");
    let arms = variants
        .iter()
        .map(op_name_arm)
        .collect::<Vec<_>>()
        .join("\n");
    let generated = format!(
        "impl Op {{\n    pub const fn variant_name(&self) -> &'static str {{\n        match self {{\n{arms}\n        }}\n    }}\n}}\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(output.join("op_variant_name.rs"), generated).expect("write Op names");
    println!("cargo:rerun-if-changed=src/ops_op.rs");
}

fn extract_op_variants(source: &str) -> Vec<(&str, &str, Option<&str>)> {
    const PREFIX: &str = "    ";
    let body = source
        .split_once("pub enum Op {")
        .expect("Op declaration")
        .1;
    let mut variants = Vec::new();
    let mut cfg = None;
    for line in body.lines().take_while(|line| *line != "}") {
        let Some(rest) = line.strip_prefix(PREFIX) else {
            continue;
        };
        if rest.starts_with("#[cfg(") {
            cfg = Some(rest);
            continue;
        }
        if rest.starts_with(' ') {
            continue;
        }
        let Some(end) =
            rest.find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        else {
            continue;
        };
        let name = &rest[..end];
        if !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|value| value.is_ascii_uppercase())
        {
            variants.push((name, rest, cfg.take()));
        }
    }
    variants
}

fn op_name_arm((name, declaration, cfg): &(&str, &str, Option<&str>)) -> String {
    let tail = declaration[name.len()..].trim_start();
    let pattern = if tail.starts_with('{') {
        format!("Self::{name} {{ .. }}")
    } else if tail.starts_with('(') {
        format!("Self::{name} (..)")
    } else {
        format!("Self::{name}")
    };
    let cfg = cfg.map_or(String::new(), |cfg| format!("            {cfg}\n"));
    format!("{cfg}            {pattern} => \"{name}\",")
}
