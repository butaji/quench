//! The host operations an image reaches, derived from one link of its code (`NativeSupport::link_reference_graph`)
//! instead of discovered by relinking: the link keeps its relocations, so every reference the code makes is an edge of
//! a graph over its symbols, and a walk from the image's entry and the operations its saved heap holds names every
//! operation whose mark (`QUENCH_OPERATION_MARK`, in the marks' own section) it can reach.
//!
//! The graph link is unoptimized, so it keeps every branch the image's fat-LTO link may fold away: its closure holds
//! the image's, and the image links once over it.

use object::{Object, ObjectSection, ObjectSymbol, RelocationKind, RelocationTarget, SymbolKind};
use std::collections::BTreeSet;
use std::path::Path;

/// The operations reached from the entry of the graph `executable`, from the `held` operations and through every
/// operation reached: an operation is reached where reached code refers to its mark, and its callback is then
/// reached, as the image's operation table (the static whose symbol ends with `table`) names it by index.
pub fn reached_operations(executable: &Path, table: &str, marks: &str, held: &BTreeSet<u32>) -> Result<BTreeSet<u32>, String> {
    let bytes = std::fs::read(executable).map_err(|error| format!("{}: {error}", executable.display()))?;
    let file = object::File::parse(&*bytes).map_err(|error| error.to_string())?;
    let marks = file.section_by_name(marks).ok_or("reference graph has no operation marks section")?;
    let marks_range = marks.address()..marks.address() + marks.size();
    let marks_data = marks.data().map_err(|error| error.to_string())?;
    let mark = |address: u64| {
        let offset = (address - marks_range.start) as usize;
        marks_data.get(offset..offset + 4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    };
    // Each relocation as (site, target): a named symbol's address, or a section-relative address with its addend
    // (a PC-relative field's addend counts from the field's end, four bytes on).
    let mut references = Vec::new();
    for section in file.sections() {
        for (site, relocation) in section.relocations() {
            let RelocationTarget::Symbol(index) = relocation.target() else { continue };
            let Ok(symbol) = file.symbol_by_index(index) else { continue };
            let target = if symbol.kind() == SymbolKind::Section {
                let pc_relative = matches!(relocation.kind(), RelocationKind::Relative | RelocationKind::PltRelative | RelocationKind::GotRelative);
                let field = u64::from(relocation.size() / 8);
                symbol.address().wrapping_add_signed(relocation.addend()).wrapping_add(if pc_relative { field } else { 0 })
            } else {
                symbol.address()
            };
            references.push((site, target));
        }
    }
    // The nodes: every defined symbol, and every referenced address no sized symbol covers (local data such as jump
    // tables and vtables, which have no symbol), each spanning up to the next node.
    let mut starts: Vec<(u64, u64)> = file.symbols()
        .filter(|symbol| symbol.is_definition() && symbol.address() != 0)
        .map(|symbol| (symbol.address(), symbol.size()))
        .collect();
    starts.sort_unstable();
    let covered = |address: u64| {
        let at = starts.partition_point(|&(start, _)| start <= address);
        at > 0 && { let (start, size) = starts[at - 1]; size > 0 && address < start + size }
    };
    let gaps: Vec<u64> = references.iter().map(|&(_, target)| target).filter(|&target| !covered(target)).collect();
    let mut nodes: Vec<u64> = starts.iter().map(|&(start, _)| start).chain(gaps).collect();
    nodes.sort_unstable();
    nodes.dedup();
    let node = |address: u64| nodes.partition_point(|&start| start <= address).checked_sub(1);
    let table = file.symbols().find(|symbol| symbol.name().is_ok_and(|name| name.ends_with(table)))
        .ok_or("reference graph has no operation table")?;
    let table_node = node(table.address());
    // Edges by source node; a reference into the marks names an operation, and the table's entries are its callbacks.
    let mut edges: Vec<Vec<Edge>> = vec![Vec::new(); nodes.len()];
    let mut callbacks = std::collections::HashMap::new();
    for &(site, target) in &references {
        let Some(source) = node(site) else { continue };
        if Some(source) == table_node && (table.address()..table.address() + table.size()).contains(&site) {
            if let Some(callback) = node(target) {
                callbacks.insert(((site - table.address()) / 8) as u32, callback);
            }
        } else if marks_range.contains(&target) {
            if let Some(operation) = mark(target) { edges[source].push(Edge::Operation(operation)); }
        } else if let Some(target) = node(target) {
            edges[source].push(Edge::Node(target));
        }
    }
    // The roots: the entry, the start and exit code, and the held operations' callbacks.
    let mut stack: Vec<usize> = node(file.entry()).into_iter().collect();
    for name in [".init", ".fini"] {
        stack.extend(file.section_by_name(name).and_then(|section| node(section.address())));
    }
    for name in [".preinit_array", ".init_array", ".fini_array"] {
        let Some(section) = file.section_by_name(name) else { continue };
        let data = section.data().map_err(|error| error.to_string())?;
        stack.extend(data.chunks_exact(8).filter_map(|word| node(u64::from_le_bytes(word.try_into().unwrap()))));
    }
    let mut reached = held.clone();
    stack.extend(held.iter().filter_map(|operation| callbacks.get(operation).copied()));
    let mut seen = vec![false; nodes.len()];
    while let Some(at) = stack.pop() {
        if std::mem::replace(&mut seen[at], true) || Some(at) == table_node {
            continue;
        }
        for edge in &edges[at] {
            match *edge {
                Edge::Node(target) => stack.push(target),
                Edge::Operation(operation) => {
                    if reached.insert(operation) {
                        stack.extend(callbacks.get(&operation).copied());
                    }
                }
            }
        }
    }
    Ok(reached)
}

#[derive(Clone, Copy)]
enum Edge {
    Node(usize),
    Operation(u32),
}
