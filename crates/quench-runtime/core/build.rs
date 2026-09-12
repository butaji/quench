#![allow(dead_code, unused_imports)]

use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget, SymbolKind};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(unexpected_cfgs)]
mod operand_holes {
    include!("stencil-aot/operand_holes.rs");
}

#[allow(unexpected_cfgs)]
mod site_holes {
    include!("stencil-aot/site_holes.rs");
}

#[allow(unexpected_cfgs)]
mod raw_value_holes {
    include!("stencil-aot/raw_value_holes.rs");
}

mod patch_schema {
    include!("stencil-aot/patch_schema.rs");
}

const PINNED_RUSTC_RELEASE: &str = "rustc 1.98.0-nightly (e7815e522 2026-06-04)";
const HANDLER_SYMBOL_PREFIX: &str = "quench_";
const NEXT_HOLE_SYMBOL: &str = "__quench_hole_next";
const DYN_NEXT_HOLE_SYMBOL: &str = "__quench_dyn_hole_next";
const DYN_SLOW_HOLE_SYMBOL: &str = "__quench_dyn_hole_slow";
const DYN_BRANCH_HOLE_SYMBOL: &str = "__quench_dyn_hole_branch";
const REGISTER_REGION_NEXT_HOLE_SYMBOL: &str = "__quench_register_region_hole_next";
const REGISTER_REGION_BRANCH_HOLE_SYMBOL: &str = "__quench_register_region_hole_branch";
const REGISTER_REGION_SLOW_HOLE_SYMBOL: &str = "__quench_register_region_hole_slow";
const REGISTER_REGION_LEAVE_HOLE_SYMBOL: &str = "__quench_register_region_hole_leave";
const GENERATED_CATALOG: &str = "stencil_catalog.rs";
const GENERATED_REGISTER_REGION: &str = "register_region_generated.rs";
const REGISTER_REGION_LANE_COUNT: usize = 4;
const STENCIL_OPT_LEVEL_ENV: &str = "QUENCH_STENCIL_OPT_LEVEL";
const STENCIL_COOKER_AUDIT_ENV: &str = "QUENCH_STENCIL_COOKER_AUDIT";
const STENCIL_COOKER_AUDIT_CFG: &str = "quench_stencil_audit_variant_c";
const STENCIL_COOKER_AUDIT_MANIFEST: &str = "stencil_cooker_audit.txt";
const DEFAULT_STENCIL_OPT_LEVEL: &str = "2";
const SUPPORTED_STENCIL_OPT_LEVELS: [&str; 4] = ["2", "3", "s", "z"];
const AARCH64_BRANCH_OPCODE_MASK: u32 = 0xfc00_0000;
const AARCH64_TAIL_BRANCH_OPCODE: u32 = 0x1400_0000;
const AARCH64_LOAD_STORE_UNSIGNED_OPCODE_MASK: u32 = 0xffc0_0000;
const AARCH64_LOAD_X_UNSIGNED_OPCODE: u32 = 0xf940_0000;
const AARCH64_STORE_X_UNSIGNED_OPCODE: u32 = 0xf900_0000;
const AARCH64_LOAD_F64_UNSIGNED_OPCODE: u32 = 0xfd40_0000;
const AARCH64_STORE_F64_UNSIGNED_OPCODE: u32 = 0xfd00_0000;
const AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT: u32 = 10;
const AARCH64_UNSIGNED_OFFSET_FIELD_MASK: u32 =
    (1_u32 << operand_holes::AARCH64_UNSIGNED_OFFSET_FIELD_BITS) - 1;
const AARCH64_ADD_IMMEDIATE_OPCODE_MASK: u32 = 0xffc0_0000;
const AARCH64_ADD_X_IMMEDIATE_OPCODE: u32 = 0x9100_0000;
const AARCH64_ADD_IMMEDIATE_FIELD_SHIFT: u32 = 10;
const AARCH64_ADD_IMMEDIATE_FIELD_MASK: u32 =
    (1_u32 << site_holes::AARCH64_ADD_IMMEDIATE_FIELD_BITS) - 1;
const AARCH64_MOV_WIDE_OPCODE_MASK: u32 = 0xff80_0000;
const AARCH64_MOV_ZERO_X_OPCODE: u32 = 0xd280_0000;
const AARCH64_MOV_KEEP_X_OPCODE: u32 = 0xf280_0000;
const AARCH64_MOV_ZERO_W_OPCODE: u32 = 0x5280_0000;
const AARCH64_MOV_KEEP_W_OPCODE: u32 = 0x7280_0000;
const AARCH64_MOV_WIDE_LANE_SHIFT: u32 = 21;
const AARCH64_MOV_WIDE_LANE_MASK: u32 = 0b11;
const AARCH64_MOV_WIDE_IMMEDIATE_SHIFT: u32 = 5;
const AARCH64_MOV_WIDE_IMMEDIATE_MASK: u32 = 0xffff;
const AARCH64_INSTRUCTION_BYTES: usize = std::mem::size_of::<u32>();
const FNV1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

fn main() {
    println!("cargo:rerun-if-changed=stencil-aot/handlers.rs");
    println!("cargo:rerun-if-changed=stencil-aot/guest_frame_schema.rs");
    println!("cargo:rerun-if-changed=stencil-aot/operand_holes.rs");
    println!("cargo:rerun-if-changed=stencil-aot/site_holes.rs");
    println!("cargo:rerun-if-changed=stencil-aot/raw_value_holes.rs");
    println!("cargo:rerun-if-changed=stencil-aot/register_region.rs");
    println!("cargo:rerun-if-changed=stencil-aot/patch_schema.rs");
    println!("cargo:rerun-if-changed=stencil-aot/object_layout.rs");
    println!("cargo:rerun-if-env-changed={STENCIL_OPT_LEVEL_ENV}");
    println!("cargo:rerun-if-env-changed={STENCIL_COOKER_AUDIT_ENV}");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let object_path = output.join("stencil_handlers.o");
    let opt_level = stencil_opt_level();
    verify_toolchain();
    generate_register_region_matrix(&output.join(GENERATED_REGISTER_REGION));
    compile_handlers(&object_path, opt_level);
    extract_catalog(&object_path, &output.join(GENERATED_CATALOG), opt_level);
    if env::var_os(STENCIL_COOKER_AUDIT_ENV).is_some() {
        audit_stencil_cooker(&object_path, &output, opt_level);
    }
}

fn generate_register_region_matrix(path: &Path) {
    const F64_BINARY_OPERATIONS: [(&str, &str); 3] =
        [("subtract", "-"), ("multiply", "*"), ("divide", "/")];
    const WORD_BINARY_OPERATIONS: [(&str, &str); 3] =
        [("bit_or", "|"), ("bit_xor", "^"), ("bit_and", "&")];
    const WORD_SHIFT_OPERATIONS: [(&str, &str); 3] = [
        ("shift_left", "left"),
        ("shift_right", "right"),
        ("shift_right_unsigned", "right_unsigned"),
    ];
    const COMPARE_OPERATIONS: [(&str, &str); 6] = [
        ("equal", "=="),
        ("not_equal", "!="),
        ("less", "<"),
        ("less_equal", "<="),
        ("greater", ">"),
        ("greater_equal", ">="),
    ];

    let mut source = String::from("// @generated by build.rs; do not edit.\n");
    for (operation, operator) in F64_BINARY_OPERATIONS {
        let _ = writeln!(source, "define_f64_binary_named!({operator};");
        write_binary_lane_rows(&mut source, operation, "d");
        source.push_str(");\n");
    }
    for (operation, operator) in WORD_BINARY_OPERATIONS {
        let _ = writeln!(source, "define_word_binary_named!({operator};");
        write_binary_lane_rows(&mut source, operation, "w");
        source.push_str(");\n");
    }
    for (operation, shift) in WORD_SHIFT_OPERATIONS {
        let _ = writeln!(source, "define_word_shift_named!({shift};");
        write_binary_lane_rows(&mut source, operation, "w");
        source.push_str(");\n");
    }
    for (operation, operator) in COMPARE_OPERATIONS {
        let _ = writeln!(source, "define_f64_compare_named!({operator};");
        for left in 0..REGISTER_REGION_LANE_COUNT {
            for right in 0..REGISTER_REGION_LANE_COUNT {
                if left == right {
                    continue;
                }
                let _ = writeln!(
                    source,
                    "    (quench_register_region_{operation}_l{left}{right}, {left}, {right}),"
                );
            }
        }
        source.push_str(");\n");
    }
    for proven in [false, true] {
        let suffix = if proven { "_proven" } else { "" };
        let helper = if proven {
            "register_region_proven_dense_index"
        } else {
            "register_region_dense_index"
        };
        let _ = writeln!(source, "define_dense_read_named!({helper};");
        for destination in 0..REGISTER_REGION_LANE_COUNT {
            for index in 0..REGISTER_REGION_LANE_COUNT {
                let _ = writeln!(
                    source,
                    "    (quench_register_region_read_dense{suffix}_d{destination}{index}, {destination}, {index}),"
                );
            }
        }
        source.push_str(");\n");
        let _ = writeln!(source, "define_dense_word_read_named!({helper};");
        for destination in 0..REGISTER_REGION_LANE_COUNT {
            for index in 0..REGISTER_REGION_LANE_COUNT {
                let _ = writeln!(
                    source,
                    "    (quench_register_region_read_dense_word{suffix}_w{destination}d{index}, {destination}, {index}),"
                );
            }
        }
        source.push_str(");\n");
        let _ = writeln!(source, "define_dense_write_named!({helper};");
        for index in 0..REGISTER_REGION_LANE_COUNT {
            for value in 0..REGISTER_REGION_LANE_COUNT {
                let _ = writeln!(
                    source,
                    "    (quench_register_region_write_dense{suffix}_i{index}{value}, {index}, {value}),"
                );
            }
        }
        source.push_str(");\n");
    }
    fs::write(path, source).expect("write register-region stencil matrix");
}

fn write_binary_lane_rows(source: &mut String, operation: &str, bank: &str) {
    for destination in 0..REGISTER_REGION_LANE_COUNT {
        for left in 0..REGISTER_REGION_LANE_COUNT {
            for right in 0..REGISTER_REGION_LANE_COUNT {
                let _ = writeln!(
                    source,
                    "    (quench_register_region_{operation}_{bank}{destination}{left}{right}, {destination}, {left}, {right}),"
                );
            }
        }
    }
}

fn stencil_opt_level() -> &'static str {
    let requested =
        env::var(STENCIL_OPT_LEVEL_ENV).unwrap_or_else(|_| DEFAULT_STENCIL_OPT_LEVEL.to_string());
    SUPPORTED_STENCIL_OPT_LEVELS
        .into_iter()
        .find(|level| *level == requested)
        .unwrap_or_else(|| {
            panic!(
                "unsupported {STENCIL_OPT_LEVEL_ENV}={requested:?}; expected one of {SUPPORTED_STENCIL_OPT_LEVELS:?}"
            )
        })
}

fn rustc_command() -> Command {
    let mut command = Command::new("rustup");
    command.args(["run", "nightly", "rustc"]);
    command
}

fn verify_toolchain() {
    let output = rustc_command()
        .arg("--version")
        .output()
        .expect("run pinned nightly rustc through rustup");
    let version = String::from_utf8(output.stdout).expect("rustc version is UTF-8");
    assert_eq!(version.trim(), PINNED_RUSTC_RELEASE, "wrong stencil rustc");
}

fn compile_handlers(object_path: &Path, opt_level: &str) {
    compile_handlers_with_variant(object_path, opt_level, false);
}

fn compile_handlers_with_variant(object_path: &Path, opt_level: &str, audit_variant_c: bool) {
    let opt_level_argument = format!("-Copt-level={opt_level}");
    let mut command = rustc_command();
    command.args([
        "--edition=2024",
        "--crate-type=lib",
        "--emit=obj",
        opt_level_argument.as_str(),
        "-Cpanic=abort",
        "-Ccodegen-units=1",
        "-Cjump-tables=no",
        "-Crelocation-model=pic",
        "-Zfunction-sections=yes",
        "--check-cfg=cfg(quench_stencil_audit_variant_c)",
    ]);
    if audit_variant_c {
        command.args(["--cfg", STENCIL_COOKER_AUDIT_CFG]);
    }
    let status = command
        .arg("-o")
        .arg(object_path)
        .arg("stencil-aot/handlers.rs")
        .status()
        .expect("compile AOT stencil handlers");
    assert!(status.success(), "AOT stencil compilation failed");
}

#[derive(Debug, PartialEq, Eq)]
struct AuditedStencil {
    bytes: Vec<u8>,
    patch_sites: Vec<patch_schema::PatchSite>,
}

fn audit_stencil_cooker(production_object: &Path, output: &Path, opt_level: &str) {
    let repeated_object = output.join("stencil_handlers_audit_b.o");
    let changed_object = output.join("stencil_handlers_audit_c.o");
    compile_handlers_with_variant(&repeated_object, opt_level, false);
    compile_handlers_with_variant(&changed_object, opt_level, true);

    let production_bytes = fs::read(production_object).expect("read audit A object");
    let repeated_bytes = fs::read(&repeated_object).expect("read audit B object");
    let changed_bytes = fs::read(&changed_object).expect("read audit C object");
    let production = audited_catalog(&production_bytes);
    let repeated = audited_catalog(&repeated_bytes);
    let changed = raw_catalog(&changed_bytes);

    assert_eq!(
        production, repeated,
        "identical cooker inputs must produce identical stencil bytes and manifests"
    );
    assert_eq!(
        production.keys().collect::<Vec<_>>(),
        changed.keys().collect::<Vec<_>>(),
        "changed-placeholder catalog must contain the same stencil symbols"
    );

    let mut audited_holes = 0usize;
    let mut changed_stencils = 0usize;
    for (name, stencil) in &production {
        let changed_bytes = &changed[name];
        assert_eq!(
            stencil.bytes.len(),
            changed_bytes.len(),
            "stencil {name} changed size when only placeholder values changed"
        );
        assert_eq!(
            stencil.bytes.len() % AARCH64_INSTRUCTION_BYTES,
            0,
            "stencil {name} is not an integral AArch64 instruction stream"
        );

        let mut allowed_masks = BTreeMap::<usize, u32>::new();
        for site in &stencil.patch_sites {
            let Some(mask) = site.encoding.changed_mask() else {
                continue;
            };
            let prior = allowed_masks.insert(site.offset, mask);
            assert!(
                prior.is_none(),
                "stencil {name} has overlapping patch sites"
            );
        }

        let mut changed_holes = BTreeMap::<usize, ()>::new();
        for instruction_offset in (0..stencil.bytes.len()).step_by(AARCH64_INSTRUCTION_BYTES) {
            let instruction_end = instruction_offset + AARCH64_INSTRUCTION_BYTES;
            let before = u32::from_le_bytes(
                stencil.bytes[instruction_offset..instruction_end]
                    .try_into()
                    .expect("AArch64 instruction bytes"),
            );
            let after = u32::from_le_bytes(
                changed_bytes[instruction_offset..instruction_end]
                    .try_into()
                    .expect("AArch64 instruction bytes"),
            );
            let difference = before ^ after;
            let allowed = allowed_masks
                .get(&instruction_offset)
                .copied()
                .unwrap_or_default();
            assert_eq!(
                difference & !allowed,
                0,
                "stencil {name} changed undeclared bits at byte offset {instruction_offset}"
            );
            if difference != 0 {
                changed_holes.insert(instruction_offset, ());
            }
        }
        for offset in allowed_masks.keys() {
            assert!(
                changed_holes.contains_key(offset),
                "stencil {name} declared hole at byte offset {offset} did not change"
            );
        }
        audited_holes += allowed_masks.len();
        changed_stencils += usize::from(!allowed_masks.is_empty());
    }

    let rustc_version = rustc_command()
        .arg("--version")
        .output()
        .expect("read audit rustc version");
    let rustc_version = String::from_utf8(rustc_version.stdout).expect("rustc version is UTF-8");
    let manifest = format!(
        "status=passed\nrustc={}\nopt_level={}\nflags={}\ncatalog_symbols={}\nchanged_stencils={}\naudited_holes={}\nobject_a_bytes={}\nobject_b_bytes={}\nobject_c_bytes={}\nobject_a_fnv1a64={:016x}\nobject_b_fnv1a64={:016x}\nobject_c_fnv1a64={:016x}\n",
        rustc_version.trim(),
        opt_level,
        "panic=abort,codegen-units=1,jump-tables=no,relocation-model=pic,function-sections=yes",
        production.len(),
        changed_stencils,
        audited_holes,
        production_bytes.len(),
        repeated_bytes.len(),
        changed_bytes.len(),
        fnv1a64(&production_bytes),
        fnv1a64(&repeated_bytes),
        fnv1a64(&changed_bytes),
    );
    fs::write(output.join(STENCIL_COOKER_AUDIT_MANIFEST), manifest)
        .expect("write stencil cooker audit manifest");
}

fn audited_catalog(object_bytes: &[u8]) -> BTreeMap<String, AuditedStencil> {
    let file = object::File::parse(object_bytes).expect("parse audited stencil object");
    let mut catalog = BTreeMap::new();
    for symbol in file.symbols().filter(|symbol| is_handler_symbol(symbol)) {
        let name = macho_name(symbol.name().expect("audited stencil symbol name"));
        let (bytes, patch_sites) = extract_stencil(&file, symbol, name);
        let prior = catalog.insert(
            name.to_string(),
            AuditedStencil {
                bytes,
                patch_sites: patch_sites
                    .into_iter()
                    .filter(|site| site.encoding != patch_schema::PatchEncoding::Branch26)
                    .collect(),
            },
        );
        assert!(prior.is_none(), "duplicate audited stencil symbol {name}");
    }
    assert!(!catalog.is_empty(), "audited stencil catalog is nonempty");
    catalog
}

fn raw_catalog(object_bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let file = object::File::parse(object_bytes).expect("parse raw stencil object");
    let mut catalog = BTreeMap::new();
    for symbol in file.symbols().filter(|symbol| is_handler_symbol(symbol)) {
        let name = macho_name(symbol.name().expect("raw stencil symbol name"));
        let prior = catalog.insert(name.to_string(), symbol_code(&file, symbol));
        assert!(prior.is_none(), "duplicate raw stencil symbol {name}");
    }
    assert!(!catalog.is_empty(), "raw stencil catalog is nonempty");
    catalog
}

fn is_handler_symbol<'data>(symbol: &object::Symbol<'data, '_, &'data [u8]>) -> bool {
    symbol.kind() == SymbolKind::Text
        && symbol.is_definition()
        && symbol
            .name()
            .ok()
            .is_some_and(|name| macho_name(name).starts_with(HANDLER_SYMBOL_PREFIX))
}

fn symbol_code<'data>(
    file: &object::File<'data>,
    symbol: object::Symbol<'data, '_, &'data [u8]>,
) -> Vec<u8> {
    let section_index = symbol.section_index().expect("stencil has a text section");
    let section = file
        .section_by_index(section_index)
        .expect("read stencil section");
    let section_data = section.data().expect("read stencil machine code");
    let start = (symbol.address() - section.address()) as usize;
    let size = if symbol.size() == 0 {
        let next_address = file
            .symbols()
            .filter(|candidate| {
                candidate.kind() == SymbolKind::Text
                    && candidate.section_index() == Some(section_index)
                    && candidate.address() > symbol.address()
            })
            .map(|candidate| candidate.address())
            .min()
            .unwrap_or(section.address() + section_data.len() as u64);
        (next_address - symbol.address()) as usize
    } else {
        symbol.size() as usize
    };
    let end = start
        .checked_add(size)
        .expect("stencil range does not overflow");
    section_data
        .get(start..end)
        .expect("stencil range is valid")
        .to_vec()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV1A_64_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV1A_64_PRIME)
    })
}

fn extract_catalog(object_path: &Path, generated_path: &Path, opt_level: &str) {
    let bytes = fs::read(object_path).expect("read AOT stencil object");
    let file = object::File::parse(bytes.as_slice()).expect("parse AOT stencil object");
    let mut symbols = file
        .symbols()
        .filter(|symbol| {
            symbol.kind() == SymbolKind::Text
                && symbol.is_definition()
                && symbol
                    .name()
                    .ok()
                    .is_some_and(|name| macho_name(name).starts_with(HANDLER_SYMBOL_PREFIX))
        })
        .collect::<Vec<_>>();
    symbols.sort_by_key(|symbol| symbol.name().unwrap_or_default().to_string());
    assert!(!symbols.is_empty(), "at least one stencil symbol exists");

    let mut generated = String::from(
        "pub struct RustcStencil {\n    pub name: &'static str,\n    pub bytes: &'static [u8],\n    pub patch_sites: &'static [crate::patch_schema::PatchSite],\n}\n",
    );
    let mut catalog_names = Vec::new();
    let mut catalog_code_bytes = 0usize;
    let mut catalog_relocations = 0usize;
    for symbol in symbols {
        let symbol_name = macho_name(symbol.name().expect("stencil symbol name"));
        let constant_name = symbol_name
            .strip_prefix(HANDLER_SYMBOL_PREFIX)
            .expect("stencil prefix")
            .to_ascii_uppercase();
        let (stencil_bytes, patch_sites) = extract_stencil(&file, symbol, symbol_name);
        catalog_code_bytes += stencil_bytes.len();
        catalog_relocations += patch_sites.len();
        writeln!(
            generated,
            "pub const {constant_name}_BYTES: &[u8] = &{stencil_bytes:?};"
        )
        .unwrap();
        writeln!(
            generated,
            "pub const {constant_name}_PATCH_SITES: &[crate::patch_schema::PatchSite] = &["
        )
        .unwrap();
        for site in &patch_sites {
            writeln!(generated, "    {},", format_patch_site(*site)).unwrap();
        }
        writeln!(generated, "];").unwrap();
        catalog_names.push((symbol_name.to_string(), constant_name));
    }
    writeln!(
        generated,
        "pub const STENCIL_COOKER_OPT_LEVEL: &str = \"{opt_level}\";"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const STENCIL_CATALOG_CODE_BYTES: usize = {catalog_code_bytes};"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const STENCIL_CATALOG_RELOCATIONS: usize = {catalog_relocations};"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const STENCIL_CATALOG_SYMBOLS: usize = {};",
        catalog_names.len()
    )
    .unwrap();
    writeln!(generated, "pub const STENCILS: &[RustcStencil] = &[").unwrap();
    for (symbol_name, constant_name) in catalog_names {
        writeln!(
            generated,
            "    RustcStencil {{ name: \"{symbol_name}\", bytes: {constant_name}_BYTES, patch_sites: {constant_name}_PATCH_SITES }},"
        )
        .unwrap();
    }
    writeln!(generated, "];").unwrap();
    fs::write(generated_path, generated).expect("write generated stencil catalog");
}

fn extract_stencil<'data>(
    file: &object::File<'data>,
    symbol: object::Symbol<'data, '_, &'data [u8]>,
    symbol_name: &str,
) -> (Vec<u8>, Vec<patch_schema::PatchSite>) {
    let section_index = symbol.section_index().expect("stencil has a text section");
    let section = file
        .section_by_index(section_index)
        .expect("read stencil section");
    let section_data = section.data().expect("read stencil machine code");
    let start = (symbol.address() - section.address()) as usize;
    let size = if symbol.size() == 0 {
        let next_address = file
            .symbols()
            .filter(|candidate| {
                candidate.kind() == SymbolKind::Text
                    && candidate.section_index() == Some(section_index)
                    && candidate.address() > symbol.address()
            })
            .map(|candidate| candidate.address())
            .min()
            .unwrap_or(section.address() + section_data.len() as u64);
        (next_address - symbol.address()) as usize
    } else {
        symbol.size() as usize
    };
    let end = start
        .checked_add(size)
        .expect("stencil range does not overflow");
    let stencil_bytes = section_data
        .get(start..end)
        .expect("stencil range is valid")
        .to_vec();

    let mut next_relocations = Vec::new();
    let mut slow_relocations = Vec::new();
    let mut branch_relocations = Vec::new();
    let mut unexpected_relocations = Vec::new();
    for (offset, relocation) in section.relocations() {
        let offset = offset as usize;
        if offset < start || offset >= end {
            continue;
        }
        let RelocationTarget::Symbol(target) = relocation.target() else {
            continue;
        };
        let target = file
            .symbol_by_index(target)
            .expect("read relocation symbol");
        if let Ok(name) = target.name() {
            let name = macho_name(name);
            if matches!(
                name,
                NEXT_HOLE_SYMBOL
                    | DYN_NEXT_HOLE_SYMBOL
                    | REGISTER_REGION_NEXT_HOLE_SYMBOL
                    | REGISTER_REGION_LEAVE_HOLE_SYMBOL
            ) {
                next_relocations.push(offset - start);
            } else if matches!(
                name,
                DYN_SLOW_HOLE_SYMBOL | REGISTER_REGION_SLOW_HOLE_SYMBOL
            ) {
                slow_relocations.push(offset - start);
            } else if matches!(
                name,
                DYN_BRANCH_HOLE_SYMBOL | REGISTER_REGION_BRANCH_HOLE_SYMBOL
            ) {
                branch_relocations.push(offset - start);
            } else {
                unexpected_relocations.push((offset - start, name.to_string()));
            }
        }
    }
    assert!(
        unexpected_relocations.is_empty(),
        "stencil contains unsupported relocations: {unexpected_relocations:?}"
    );
    assert_eq!(
        next_relocations.len(),
        1,
        "stencil {symbol_name} must have one next hole"
    );
    assert!(
        slow_relocations.len() <= 1,
        "stencil has at most one slow hole"
    );
    assert!(
        branch_relocations.len() <= 1,
        "stencil has at most one taken-branch hole"
    );
    validate_tail_branch(&stencil_bytes, next_relocations[0], "next continuation");
    if let Some(slow_relocation) = slow_relocations.first().copied() {
        validate_tail_branch(&stencil_bytes, slow_relocation, "slow continuation");
    }
    if let Some(branch_relocation) = branch_relocations.first().copied() {
        validate_tail_branch(&stencil_bytes, branch_relocation, "taken continuation");
    }
    let operand_relocations = match operand_holes::expected_kinds_for_stencil(symbol_name) {
        Some(expected_kinds) => {
            let relocations = extract_operand_relocations(&stencil_bytes);
            validate_operand_relocation_manifest(symbol_name, expected_kinds, &relocations);
            relocations
        }
        None => {
            assert!(
                !symbol_name.contains("_burned_"),
                "burned stencil {symbol_name} is missing an operand-hole manifest"
            );
            Vec::new()
        }
    };
    let site_advance_relocations = extract_site_advance_relocations(&stencil_bytes);
    assert!(
        site_advance_relocations.len() <= 1,
        "stencil {symbol_name} has more than one site-advance hole"
    );
    let mut patch_sites = vec![patch_schema::PatchSite {
        offset: next_relocations[0],
        encoding: patch_schema::PatchEncoding::Branch26,
        binding: patch_schema::PatchBinding::Next,
    }];
    patch_sites.extend(
        slow_relocations
            .first()
            .map(|offset| patch_schema::PatchSite {
                offset: *offset,
                encoding: patch_schema::PatchEncoding::Branch26,
                binding: patch_schema::PatchBinding::Slow,
            }),
    );
    patch_sites.extend(
        branch_relocations
            .first()
            .map(|offset| patch_schema::PatchSite {
                offset: *offset,
                encoding: patch_schema::PatchEncoding::Branch26,
                binding: patch_schema::PatchBinding::Taken,
            }),
    );
    patch_sites.extend(operand_relocations.into_iter().map(|(offset, kind)| {
        patch_schema::PatchSite {
            offset,
            encoding: patch_schema::PatchEncoding::LoadStoreUnsigned12,
            binding: patch_schema::PatchBinding::Operand(kind),
        }
    }));
    patch_sites.extend(site_advance_relocations.into_iter().map(|offset| {
        patch_schema::PatchSite {
            offset,
            encoding: patch_schema::PatchEncoding::AddImmediate12,
            binding: patch_schema::PatchBinding::SiteAdvanceBytes,
        }
    }));
    patch_sites.extend(extract_raw_value_patch_sites(symbol_name, &stencil_bytes));
    patch_sites.sort_by_key(|site| site.offset);
    (stencil_bytes, patch_sites)
}

fn extract_raw_value_patch_sites(
    stencil_name: &str,
    stencil_bytes: &[u8],
) -> Vec<patch_schema::PatchSite> {
    let Some(spec) = raw_value_holes::expected_raw_value_hole(stencil_name) else {
        return Vec::new();
    };
    let mut lanes = BTreeMap::new();
    for (instruction_index, bytes) in stencil_bytes
        .chunks_exact(AARCH64_INSTRUCTION_BYTES)
        .enumerate()
    {
        let instruction = u32::from_le_bytes(bytes.try_into().expect("AArch64 instruction"));
        let opcode = instruction & AARCH64_MOV_WIDE_OPCODE_MASK;
        if !matches!(
            opcode,
            AARCH64_MOV_ZERO_X_OPCODE
                | AARCH64_MOV_KEEP_X_OPCODE
                | AARCH64_MOV_ZERO_W_OPCODE
                | AARCH64_MOV_KEEP_W_OPCODE
        ) {
            continue;
        }
        let lane = (instruction >> AARCH64_MOV_WIDE_LANE_SHIFT) & AARCH64_MOV_WIDE_LANE_MASK;
        let immediate =
            (instruction >> AARCH64_MOV_WIDE_IMMEDIATE_SHIFT) & AARCH64_MOV_WIDE_IMMEDIATE_MASK;
        let expected = ((raw_value_holes::RAW_VALUE_HOLE_BITS
            >> (lane * raw_value_holes::RAW_VALUE_LANE_BITS))
            & u64::from(AARCH64_MOV_WIDE_IMMEDIATE_MASK)) as u32;
        let lane_is_present = spec.lane_mask & (1 << lane) != 0;
        if lane_is_present && immediate == expected {
            let prior = lanes.insert(lane, instruction_index * AARCH64_INSTRUCTION_BYTES);
            assert!(
                prior.is_none(),
                "stencil {stencil_name} repeats raw-value lane {lane}"
            );
        }
    }
    assert_eq!(
        lanes.len(),
        spec.lane_mask.count_ones() as usize,
        "stencil {stencil_name} must materialize every raw-value lane"
    );
    (0..raw_value_holes::RAW_VALUE_LANE_COUNT as u32)
        .filter(|lane| spec.lane_mask & (1 << lane) != 0)
        .map(|lane| patch_schema::PatchSite {
            offset: *lanes
                .get(&lane)
                .unwrap_or_else(|| panic!("stencil {stencil_name} is missing lane {lane}")),
            encoding: patch_schema::PatchEncoding::MovWide16,
            binding: patch_schema::PatchBinding::RawValue { id: spec.id },
        })
        .collect()
}

fn extract_site_advance_relocations(stencil_bytes: &[u8]) -> Vec<usize> {
    stencil_bytes
        .chunks_exact(std::mem::size_of::<u32>())
        .enumerate()
        .filter_map(|(instruction_index, bytes)| {
            let instruction = u32::from_le_bytes(bytes.try_into().unwrap());
            let opcode = instruction & AARCH64_ADD_IMMEDIATE_OPCODE_MASK;
            let immediate = (instruction >> AARCH64_ADD_IMMEDIATE_FIELD_SHIFT)
                & AARCH64_ADD_IMMEDIATE_FIELD_MASK;
            (opcode == AARCH64_ADD_X_IMMEDIATE_OPCODE
                && immediate as usize == site_holes::NEXT_SITE_BYTE_OFFSET_HOLE)
                .then_some(instruction_index * std::mem::size_of::<u32>())
        })
        .collect()
}

fn extract_operand_relocations(
    stencil_bytes: &[u8],
) -> Vec<(usize, operand_holes::OperandHoleKind)> {
    stencil_bytes
        .chunks_exact(std::mem::size_of::<u32>())
        .enumerate()
        .filter_map(|(instruction_index, bytes)| {
            let instruction = u32::from_le_bytes(bytes.try_into().unwrap());
            let opcode = instruction & AARCH64_LOAD_STORE_UNSIGNED_OPCODE_MASK;
            if !matches!(
                opcode,
                AARCH64_LOAD_X_UNSIGNED_OPCODE
                    | AARCH64_STORE_X_UNSIGNED_OPCODE
                    | AARCH64_LOAD_F64_UNSIGNED_OPCODE
                    | AARCH64_STORE_F64_UNSIGNED_OPCODE
            ) {
                return None;
            }
            let scaled_offset = (instruction >> AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT)
                & AARCH64_UNSIGNED_OFFSET_FIELD_MASK;
            operand_holes::kind_for_placeholder_slot(scaled_offset as usize)
                .map(|kind| (instruction_index * std::mem::size_of::<u32>(), kind))
        })
        .collect()
}

fn operand_hole_variant(kind: operand_holes::OperandHoleKind) -> &'static str {
    use operand_holes::OperandHoleKind::*;
    match kind {
        DestinationRegisterByteOffset => "DestinationRegisterByteOffset",
        SourceRegisterByteOffset => "SourceRegisterByteOffset",
        LeftRegisterByteOffset => "LeftRegisterByteOffset",
        RightRegisterByteOffset => "RightRegisterByteOffset",
        DestinationLocalByteOffset => "DestinationLocalByteOffset",
        SourceLocalByteOffset => "SourceLocalByteOffset",
    }
}

fn format_patch_site(site: patch_schema::PatchSite) -> String {
    use patch_schema::{PatchBinding, PatchEncoding};
    let encoding = match site.encoding {
        PatchEncoding::Branch26 => "Branch26".to_string(),
        PatchEncoding::LoadStoreUnsigned12 => "LoadStoreUnsigned12".to_string(),
        PatchEncoding::AddImmediate12 => "AddImmediate12".to_string(),
        PatchEncoding::MovWide16 => "MovWide16".to_string(),
        PatchEncoding::RawWord32 => "RawWord32".to_string(),
        PatchEncoding::Pointer64 => "Pointer64".to_string(),
    };
    let binding = match site.binding {
        PatchBinding::Next => "Next".to_string(),
        PatchBinding::Slow => "Slow".to_string(),
        PatchBinding::Taken => "Taken".to_string(),
        PatchBinding::SiteAdvanceBytes => "SiteAdvanceBytes".to_string(),
        PatchBinding::RawValue { id } => format!("RawValue {{ id: {id} }}"),
        PatchBinding::Operand(kind) => {
            format!(
                "Operand(crate::operand_holes::OperandHoleKind::{})",
                operand_hole_variant(kind)
            )
        }
    };
    format!(
        "crate::patch_schema::PatchSite {{ offset: {}, encoding: crate::patch_schema::PatchEncoding::{encoding}, binding: crate::patch_schema::PatchBinding::{binding} }}",
        site.offset
    )
}

fn validate_operand_relocation_manifest(
    stencil_name: &str,
    expected_kinds: &[operand_holes::OperandHoleKind],
    relocations: &[(usize, operand_holes::OperandHoleKind)],
) {
    assert_eq!(
        relocations.len(),
        expected_kinds.len(),
        "stencil {stencil_name} has the wrong operand-hole occurrence count"
    );
    for expected_kind in expected_kinds {
        let expected_count = expected_kinds
            .iter()
            .filter(|kind| *kind == expected_kind)
            .count();
        let actual_count = relocations
            .iter()
            .filter(|(_, kind)| kind == expected_kind)
            .count();
        assert_eq!(
            actual_count, expected_count,
            "stencil {stencil_name} has the wrong {expected_kind:?} occurrence count"
        );
    }
}

fn validate_tail_branch(stencil_bytes: &[u8], branch_offset: usize, kind: &str) {
    let branch_bytes: [u8; std::mem::size_of::<u32>()] = stencil_bytes
        [branch_offset..branch_offset + std::mem::size_of::<u32>()]
        .try_into()
        .expect("next relocation covers one AArch64 instruction");
    let branch = u32::from_le_bytes(branch_bytes);
    assert_eq!(
        branch & AARCH64_BRANCH_OPCODE_MASK,
        AARCH64_TAIL_BRANCH_OPCODE,
        "{kind} must be a tail branch, not a call"
    );
}

fn macho_name(name: &str) -> &str {
    name.strip_prefix('_').unwrap_or(name)
}
