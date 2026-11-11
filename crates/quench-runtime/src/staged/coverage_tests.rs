//! The coverage tests: which opcodes the staged layer decodes, and that tiers derive from it.
/// Hand-written decodes of the object and narrow layouts outside `staged`, per native-tier
/// file: each is a second home of a rule `staged` owns (AGENTS.md, manifesto 4). The counts
/// only go down; a new decode belongs in `staged` as an operation every tier runs.
const LAYOUT_DECODES: &[(&str, &str, usize)] = &[
    ("vm/compiled.rs", include_str!("../vm/compiled.rs"), 0),
    ("vm/compiled/lane_region.rs", include_str!("../vm/compiled/lane_region.rs"), 0),
    ("vm/compiled/osr_loop.rs", include_str!("../vm/compiled/osr_loop.rs"), 0),
    ("vm/compiled/object_slots.rs", include_str!("../vm/compiled/object_slots.rs"), 0),
    ("vm/js_compiled.rs", include_str!("../vm/js_compiled.rs"), 0),
    ("vm/dispatch_fast.rs", include_str!("../vm/dispatch_fast.rs"), 0),
];

/// The layout facts whose use is a decode: the bits a reader tests to learn a storage form or
/// a narrow value's meaning.
const LAYOUT_TOKENS: &[&str] = &[
    "extras_tag_mask",
    "inline_storage_mask",
    "storage_start_mask",
    "NarrowLayout::INTEGER_FLAG",
    "NarrowLayout::CODE_SHIFT",
    "proto_mask",
    "proto_record_base",
];

#[test]
fn staged_coverage_layout_decodes_outside_staged_only_shrink() {
    for (file, source, allowed) in LAYOUT_DECODES {
        let found: usize = LAYOUT_TOKENS.iter().map(|token| source.matches(token).count()).sum();
        assert!(found <= *allowed,
            "{file} decodes object or narrow layouts {found} times (allowed {allowed}): route the read through `staged`");
        assert!(found >= *allowed,
            "{file} now decodes layouts {found} times, below {allowed}: lower its allowance in LAYOUT_DECODES");
    }
}

/// The interpreters: the one place an opcode's meaning is written by hand.
const INTERPRETERS: &[&str] = &["vm/dispatch.rs", "vm/dispatch_wasm.rs", "vm/dispatch_wasm/gc.rs"];

/// Opcode names per file and opcode (`Op::X` anywhere: a match arm, a comparison, a shape
/// table) in `vm/`, outside the interpreters and tests. A tier that names an opcode decides
/// something about its meaning by hand; one that reads the opcode table's facts
/// (`control_flow_layout`, `input_binding_times`, `field_store`) derives it.
fn opcode_arms() -> std::collections::BTreeMap<(String, String), usize> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![root.join("vm")];
    let mut arms = std::collections::BTreeMap::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            let file = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.ends_with(".rs") || name.ends_with("tests.rs") || INTERPRETERS.contains(&file.as_str()) { continue; }
            count_opcode_names(&std::fs::read_to_string(&path).unwrap(), &file, &mut arms);
        }
    }
    arms
}

/// Adds `source`'s opcode names, outside an inline test module, to `arms` under `file`.
fn count_opcode_names(source: &str, file: &str, arms: &mut std::collections::BTreeMap<(String, String), usize>) {
    // Any inline test module (whatever its name) closes the production code; its fixtures name
    // opcodes freely.
    let code = source.split("\n#[cfg(test)]\nmod ").next().unwrap();
    let code = code.split("#[cfg(test)]\nmod ").next().unwrap();
    for (at, _) in code.match_indices("Op::") {
        let op: String = code[at + 4..].chars().take_while(char::is_ascii_alphanumeric).collect();
        let qualified = code[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        // `WideInstruction::new(Op::X, ...)` supplies an opcode value to the builder; it is
        // not a second definition of the opcode's meaning.
        if code[..at].trim_end().ends_with("WideInstruction::new(") { continue; }
        if !qualified && op.starts_with(|c: char| c.is_ascii_uppercase()) {
            *arms.entry((file.to_owned(), op)).or_insert(0) += 1;
        }
    }
}

/// Futamura's first projection, enforced: a tier's meaning of an opcode is the interpreter's,
/// specialized (`staged`, semantic entries), so an interpreter change reaches every tier. An
/// opcode meaning a tier writes by hand is a second definition that can drift; the table of
/// them only shrinks, and a new one fails here.
#[test]
fn tiers_derive_opcode_meaning_from_the_interpreter() {
    let allowed: std::collections::BTreeMap<(String, String), usize> = include_str!("hand_semantics.txt").lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split(' ');
            let (file, op, count) = (fields.next().unwrap(), fields.next().unwrap(), fields.next().unwrap());
            ((file.to_owned(), op.to_owned()), count.parse().unwrap())
        })
        .collect();
    let found = opcode_arms();
    for ((file, op), count) in &found {
        let limit = allowed.get(&(file.clone(), op.clone())).copied().unwrap_or(0);
        assert!(*count <= limit, "{file} names Op::{op} {count} times (allowed {limit}): derive its meaning from the \
            interpreter (`staged`, a semantic entry, an opcode-table fact) instead");
    }
    for ((file, op), limit) in &allowed {
        let count = found.get(&(file.clone(), op.clone())).copied().unwrap_or(0);
        assert!(count >= *limit, "{file} now names Op::{op} {count} times: lower its line in \
            staged/hand_semantics.txt to {count} (or drop it at 0)");
    }
}

