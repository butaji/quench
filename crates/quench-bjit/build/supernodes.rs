//! The supernode list, generated from `supernodes.txt` (`stencils/supernodes.rs` includes the
//! generated `FAMILIES` and `SUPERNODES`). Each line is read and validated here: every opcode name
//! a label or a family line names must be an opcode of the runtime's table (the `opcodes!` entries
//! of `quench-runtime/src/bytecode.rs`), every pattern must be one the rules build, and its feeds,
//! place sets and evidence must be named. A bad line is a build error, with the line in the message.
//! The rules (`stencils/supernode_rules.rs`) decide what a pattern means; this file only reads the
//! data and writes the list's Rust text.

use crate::supernode_rules::{self as rules, Family, MAX_SUPERNODES, MAX_VARIANTS, NODES, Node, Supernode};
use std::{fs, path::Path};

/// The checked-in list.
const LIST: &str = include_str!("../supernodes.txt");
/// The runtime's opcode table, relative to the crate directory.
const OPCODE_TABLE: &str = "../quench-runtime/src/bytecode.rs";
/// The line that opens the table's `opcodes!` invocation, and the line that closes it.
const TABLE_START: &str = "opcodes!($;";
const TABLE_END: &str = ");";
/// An entry of the table: four spaces, the opcode's name, then its arrow.
const TABLE_INDENT: &str = "    ";
const TABLE_ARROW: &str = " => ";
/// The prefix of a family line, and the separator of an entry's fields.
const FAMILY_PREFIX: &str = "opcode ";
const FIELD_SEPARATOR: char = '|';
/// The separator of a pattern's labels, and the mark before its links.
const LABEL_SEPARATOR: &str = " ; ";
const LINKS_OPEN: &str = " [";

/// The list as the build reads it.
pub struct Listed {
    /// The opcodes the labels may name, each with its row family, in file order.
    pub families: Vec<(String, Family)>,
    /// The entries, in file order.
    pub supernodes: Vec<Supernode>,
    /// The Rust text of `FAMILIES` and `SUPERNODES`, which `QUENCH_SUPERNODES` names.
    pub text: String,
}

impl Listed {
    /// The families in the form the rules take them.
    fn family_refs(&self) -> Vec<(&str, Family)> {
        self.families.iter().map(|(name, family)| (name.as_str(), *family)).collect()
    }
}

/// Reads and validates the list; `manifest` is the crate directory, where the opcode table is.
pub fn load(manifest: &Path) -> Listed {
    let table = fs::read_to_string(manifest.join(OPCODE_TABLE)).expect("read the runtime's opcode table");
    let opcodes = opcode_names(&table);
    let mut families = Vec::new();
    let mut entries = Vec::new();
    for line in LIST.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        match line.strip_prefix(FAMILY_PREFIX) {
            Some(rest) => families.push(family(line, rest, &opcodes)),
            None => entries.push(line),
        }
    }
    let refs: Vec<(&str, Family)> = families.iter().map(|(name, family)| (name.as_str(), *family)).collect();
    let (supernodes, lines): (Vec<Supernode>, Vec<String>) = entries.into_iter().map(|line| entry(line, &opcodes, &refs)).unzip();
    assert!(supernodes.len() <= MAX_SUPERNODES, "supernodes.txt holds {} entries, over the budget of {MAX_SUPERNODES}", supernodes.len());
    let variants = rules::instances(&supernodes, &refs).len();
    assert!(variants <= MAX_VARIANTS, "supernodes.txt builds {variants} variants, over the budget of {MAX_VARIANTS}");
    Listed { text: render(&families, &lines), families, supernodes }
}

/// The rows the stencil library builds for the list: one per variant of each entry.
pub fn instances(listed: &Listed) -> Vec<[Node; NODES]> {
    rules::instances(&listed.supernodes, &listed.family_refs())
}

/// The names of the opcodes of the table: the names of its entries, `Name => ...`, between `TABLE_START` and `TABLE_END`.
fn opcode_names(table: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in table.lines() {
        if line.starts_with(TABLE_START) {
            inside = true;
        } else if inside && line == TABLE_END {
            break;
        } else if let (true, Some((name, _))) = (inside, line.strip_prefix(TABLE_INDENT).and_then(|rest| rest.split_once(TABLE_ARROW))) {
            if name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                names.push(name.to_string());
            }
        }
    }
    assert!(!names.is_empty(), "no opcode entries found in {OPCODE_TABLE}");
    names
}

/// A family line's `<name> <family>`: the name must be an opcode of the table.
fn family(line: &str, rest: &str, opcodes: &[String]) -> (String, Family) {
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let &[name, text] = fields.as_slice() else { panic!("supernodes.txt: `{line}` is not `opcode <name> <family>`") };
    assert!(opcodes.iter().any(|opcode| opcode == name), "supernodes.txt: `{name}` is no opcode of the table");
    let family = Family::parse(text).unwrap_or_else(|| panic!("supernodes.txt: `{line}` names no row family"));
    (name.to_string(), family)
}

/// An entry line, `pattern | feeds | first places | second places | evidence`, as the supernode it
/// names and its line of the generated list.
fn entry(line: &'static str, opcodes: &[String], families: &[(&str, Family)]) -> (Supernode, String) {
    let fields: Vec<&str> = line.split(FIELD_SEPARATOR).map(str::trim).collect();
    let &[pattern, feeds, first, second, evidence] = fields.as_slice() else {
        panic!("supernodes.txt: `{line}` is not `pattern | feeds | first places | second places | evidence`")
    };
    let labels = pattern.rsplit_once(LINKS_OPEN).map_or(pattern, |(labels, _)| labels);
    for label in labels.split(LABEL_SEPARATOR) {
        let name = rules::opcode_of_label(label);
        assert!(opcodes.iter().any(|opcode| opcode == name), "supernodes.txt: `{name}` in `{pattern}` is no opcode of the table");
    }
    assert!(rules::parse(pattern, families).is_some(), "supernodes.txt: `{pattern}` is not a pattern the library builds");
    let feeds = rules::feeds_of(feeds).unwrap_or_else(|| panic!("supernodes.txt: `{line}` names no feeds of A or B"));
    let place = |name: &str| rules::place_class(name).unwrap_or_else(|| panic!("supernodes.txt: `{name}` is no place set"));
    assert!(!evidence.is_empty(), "supernodes.txt: `{pattern}` has no evidence");
    let supernode = Supernode { pattern, feeds: feeds.clone().leak(), places: [place(first), place(second)], evidence };
    let feed_text: Vec<String> = feeds.iter().map(|feed| format!("Operand::{feed:?}")).collect();
    let text = format!("    Supernode {{ pattern: {pattern:?}, feeds: &[{}], places: [{first}, {second}], evidence: {evidence:?} }},\n", feed_text.join(", "));
    (supernode, text)
}

/// The generated file: `FAMILIES`, then `SUPERNODES` with the entries' lines.
fn render(families: &[(String, Family)], lines: &[String]) -> String {
    let mut text = String::from("pub static FAMILIES: &[(&str, Family)] = &[\n");
    for (name, family) in families {
        text += &format!("    ({name:?}, Family::{family:?}),\n");
    }
    text += "];\n\npub static SUPERNODES: &[Supernode] = &[\n";
    for line in lines {
        text += line;
    }
    text += "];\n";
    text
}
