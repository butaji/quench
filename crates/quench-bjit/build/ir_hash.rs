//! Content keys of the stencil rows, read from the LLVM IR of their shard (`ir_text`). A row's key hashes its
//! normalized definition and everything the definition reaches (functions, globals, types, attribute groups and
//! metadata it names), so the same row keys the same in any shard, and a change anywhere it reaches changes the
//! key. Keys are a Merkle hash over the reachable items, computed in reverse topological order of their strongly
//! connected components (Tarjan). A recursive group is keyed as a unit: each member's label is refined over the
//! group's references, and every member's key hashes the group's labels, so two groups with the same bodies but
//! different wiring key differently.

use crate::cache::Fnv;
use crate::ir_text::{Kind, Module, Seg, normalize, parse, prelude_key_text};

/// Canonical hash of a segment list: literal text framed by length, item references by their key.
fn digest(segs: &[Seg], key_of: impl Fn(usize) -> u128) -> u128 {
    let mut hash = Fnv::default();
    for seg in segs {
        match seg {
            Seg::Text(bytes) => {
                hash.bytes(&[0]);
                hash.field(bytes);
            }
            Seg::Item(id) => {
                hash.bytes(&[1]);
                hash.bytes(&key_of(*id).to_le_bytes());
            }
        }
    }
    hash.finish()
}

const UNSEEN: usize = usize::MAX;

/// Tarjan's walk over the items reachable from the roots, keying each strongly connected component when it closes:
/// a component's members are keyed after everything they reach outside it.
struct Keys<'m, 'a> {
    module: &'m Module<'a>,
    index: Vec<usize>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    segs: Vec<Vec<Seg>>,
    refs: Vec<Vec<usize>>,
    key: Vec<u128>,
    /// The component each item closed in (`group`) and its position there (`pos`).
    group: Vec<usize>,
    pos: Vec<usize>,
    groups: usize,
    counter: usize,
}

impl<'m, 'a> Keys<'m, 'a> {
    fn new(module: &'m Module<'a>) -> Self {
        let n = module.items.len();
        Self {
            module,
            index: vec![UNSEEN; n],
            low: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            segs: (0..n).map(|_| Vec::new()).collect(),
            refs: (0..n).map(|_| Vec::new()).collect(),
            key: vec![0; n],
            group: vec![UNSEEN; n],
            pos: vec![0; n],
            groups: 0,
            counter: 0,
        }
    }

    /// Enters `v`: numbers it, normalizes its text, and pushes it on the walk's stack.
    fn enter(&mut self, v: usize) {
        self.index[v] = self.counter;
        self.low[v] = self.counter;
        self.counter += 1;
        self.on_stack[v] = true;
        self.stack.push(v);
        let segs = normalize(self.module, &self.module.items[v]);
        self.refs[v] = segs.iter().filter_map(|seg| if let Seg::Item(id) = seg { Some(*id) } else { None }).collect();
        self.segs[v] = segs;
    }

    fn walk(&mut self, root: usize) {
        self.enter(root);
        let mut calls: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(v, edge)) = calls.last() {
            if let Some(&w) = self.refs[v].get(edge) {
                calls.last_mut().expect("a call").1 += 1;
                if self.index[w] == UNSEEN {
                    self.enter(w);
                    calls.push((w, 0));
                } else if self.on_stack[w] {
                    self.low[v] = self.low[v].min(self.index[w]);
                }
            } else {
                calls.pop();
                if self.low[v] == self.index[v] {
                    self.close(v);
                }
                if let Some(&(u, _)) = calls.last() {
                    self.low[u] = self.low[u].min(self.low[v]);
                }
            }
        }
    }

    /// Keys the component whose root is `root`, the members above it on the stack.
    fn close(&mut self, root: usize) {
        let mut members = Vec::new();
        loop {
            let w = self.stack.pop().expect("a member on the stack");
            self.on_stack[w] = false;
            members.push(w);
            if w == root {
                break;
            }
        }
        if members.len() == 1 && !self.refs[root].contains(&root) {
            let key = digest(&self.segs[root], |id| self.key[id]);
            self.key[root] = key;
        } else {
            self.refine(&members);
        }
        for &m in &members {
            self.segs[m] = Vec::new();
        }
    }

    /// Keys a recursive component. Labels start from each member's text with the component's references blanked, and
    /// are refined over the references for as many rounds as there are members (enough for the partition to settle).
    fn refine(&mut self, members: &[usize]) {
        let gid = self.groups;
        self.groups += 1;
        for (p, &m) in members.iter().enumerate() {
            self.group[m] = gid;
            self.pos[m] = p;
        }
        let this = &*self;
        let value = |id: usize, labels: &[u128]| if this.group[id] == gid { labels[this.pos[id]] } else { this.key[id] };
        let mut labels: Vec<u128> = members.iter().map(|&m| digest(&this.segs[m], |id| if this.group[id] == gid { 0 } else { this.key[id] })).collect();
        for _ in 0..members.len() {
            let next: Vec<u128> = members
                .iter()
                .enumerate()
                .map(|(p, &m)| {
                    let mut hash = Fnv::default();
                    hash.bytes(&labels[p].to_le_bytes());
                    for &id in &this.refs[m] {
                        hash.bytes(&value(id, &labels).to_le_bytes());
                    }
                    hash.finish()
                })
                .collect();
            labels = next;
        }
        let finals: Vec<u128> = members
            .iter()
            .enumerate()
            .map(|(p, &m)| {
                let body = digest(&this.segs[m], |id| value(id, &labels));
                let mut hash = Fnv::default();
                hash.bytes(&labels[p].to_le_bytes());
                hash.bytes(&body.to_le_bytes());
                hash.finish()
            })
            .collect();
        for (&m, key) in members.iter().zip(finals) {
            self.key[m] = key;
        }
    }
}

/// The key of every item reachable from `roots` (the others stay 0).
fn keys(module: &Module<'_>, roots: &[usize]) -> Vec<u128> {
    let mut walk = Keys::new(module);
    for &root in roots {
        if walk.index[root] == UNSEEN {
            walk.walk(root);
        }
    }
    walk.key
}

/// Normalized module-level text (the prelude). It names no item, so it is a plain digest.
fn prelude_key(module: &Module<'_>) -> u128 {
    digest(&prelude_key_text(module), |_| unreachable!("the prelude names no definition"))
}

/// The key of every row a shard's IR defines: a function whose name `is_row` accepts. A key covers the row's
/// normalized definition, everything it reaches, the module's prelude, `context` (the toolchain, target, cpu, flags
/// and family the caller builds with) and the row's name, which the row's table entry carries.
pub fn row_keys(ll: &str, context: &[u8], is_row: impl Fn(&str) -> bool) -> Vec<(String, u128)> {
    let module = parse(ll);
    let prelude = prelude_key(&module);
    let rows: Vec<usize> = (0..module.items.len()).filter(|&id| module.items[id].kind == Kind::Function && is_row(module.items[id].name)).collect();
    let keys = keys(&module, &rows);
    rows.iter()
        .map(|&row| {
            let name = module.items[row].name;
            let mut hash = Fnv::default();
            hash.field(context);
            hash.bytes(&prelude.to_le_bytes());
            hash.field(name.as_bytes());
            hash.bytes(&keys[row].to_le_bytes());
            (name.to_string(), hash.finish())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_text::Norm;
    use std::collections::HashMap;

    /// A module of the given definitions (each a complete `define`), with a fixed prelude.
    fn module(defs: &[&str]) -> String {
        let mut text = String::from("; ModuleID = 'shard'\nsource_filename = \"shard\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n");
        text += "!llvm.module.flags = !{!0}\n!0 = !{i32 2, !\"RtLibUseGOT\", i32 1}\n";
        text += "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)\n";
        for def in defs {
            text += def;
            text += "\n";
        }
        text
    }

    fn keys_of(text: &str) -> HashMap<String, u128> {
        row_keys(text, b"ctx", |name| !name.starts_with("_R")).into_iter().collect()
    }

    /// A row that calls `helper` (a defined function of the module) and copies through a local.
    fn row(name: &str, helper: &str) -> String {
        format!(
            "define dso_local preserve_nonecc i64 @{name}(i64 %frame, i64 %p0) unnamed_addr #1 {{\nstart:\n  %0 = alloca [16 x i8], align 8\n  %v = call i64 @{helper}(i64 %p0) #2\n  %r = add i64 %v, 1\n  ret i64 %r\n}}"
        )
    }

    fn helper(name: &str, add: u32) -> String {
        format!("define internal i64 @{name}(i64 %x) unnamed_addr #0 {{\n  %y = add i64 %x, {add}\n  ret i64 %y\n}}")
    }

    const ATTRIBUTES: &str = "attributes #0 = { inlinehint nounwind }\nattributes #1 = { nounwind }\nattributes #2 = { nounwind }\n";

    #[test]
    fn names_of_locals_and_items_do_not_change_keys() {
        let a = module(&[&row("r", "h"), &helper("h", 7), ATTRIBUTES]);
        let renamed = module(&[&row("r", "h").replace("%v", "%value").replace("%0", "%_9"), &helper("h", 7).replace("%y", "%sum"), ATTRIBUTES]);
        assert_eq!(keys_of(&a)["r"], keys_of(&renamed)["r"]);
    }

    #[test]
    fn a_change_in_a_callee_changes_the_caller() {
        let base = keys_of(&module(&[&row("r", "h"), &helper("h", 7), ATTRIBUTES]));
        let changed = keys_of(&module(&[&row("r", "h"), &helper("h", 8), ATTRIBUTES]));
        assert_ne!(base["r"], changed["r"]);
        let other_row = keys_of(&module(&[&row("q", "h"), &helper("h", 7), ATTRIBUTES]));
        assert_ne!(base["r"], other_row["q"], "the row's name is part of its key");
    }

    #[test]
    fn a_row_keys_the_same_in_any_shard() {
        // Shard A holds the row with its helper; shard B holds the same row beside an unrelated row, a helper of
        // its own, different numbering of attributes, metadata and anonymous constants, and another module name.
        let a = module(&[&row("r", "h"), &helper("h", 7), ATTRIBUTES]);
        let b = module(&[
            &row("s", "k").replace("%0", "%_0"),
            &helper("k", 3),
            &row("r", "h"),
            &helper("h", 7),
            "attributes #0 = { inlinehint nounwind }\nattributes #5 = { nounwind }\nattributes #9 = { nounwind }\nattributes #7 = { inlinehint nounwind }\n",
            "@anon.1f.4 = private unnamed_addr constant [4 x i8] c\"\\00\\00\\00\\01\", align 1\n",
        ]);
        let b = b.replace("#1 {", "#5 {").replace("#2\n", "#9\n");
        assert_eq!(keys_of(&a)["r"], keys_of(&b)["r"]);
    }

    #[test]
    fn recursive_groups_key_by_wiring() {
        let caller = row("r", "f");
        let ring = module(&[
            &caller,
            "define internal i64 @f(i64 %x) unnamed_addr #0 {\n  %y = call i64 @g(i64 %x)\n  ret i64 %y\n}",
            "define internal i64 @g(i64 %x) unnamed_addr #0 {\n  %y = call i64 @f(i64 %x)\n  ret i64 %y\n}",
            ATTRIBUTES,
        ]);
        let selfish = module(&[
            &caller,
            "define internal i64 @f(i64 %x) unnamed_addr #0 {\n  %y = call i64 @f(i64 %x)\n  ret i64 %y\n}",
            "define internal i64 @g(i64 %x) unnamed_addr #0 {\n  %y = call i64 @g(i64 %x)\n  ret i64 %y\n}",
            ATTRIBUTES,
        ]);
        assert_ne!(keys_of(&ring)["r"], keys_of(&selfish)["r"]);
        let ring_changed = ring.replace("%y = call i64 @f(i64 %x)", "%y = call i64 @f(i64 %x)\n  %z = add i64 %y, 1");
        assert_ne!(keys_of(&ring)["r"], keys_of(&ring_changed)["r"]);
    }

    #[test]
    fn undefined_names_stay_literal() {
        let base = module(&[&row("r", "h"), &helper("h", 7), ATTRIBUTES]);
        let other = base.replace("declare void @llvm.memcpy", "declare void @llvm.memmove");
        let uses = "define dso_local i64 @u() unnamed_addr #1 {\n  %v = call i64 @HOLE_NEXT(i64 1) #2\n  ret i64 %v\n}";
        let hole_a = module(&[uses, ATTRIBUTES]).replace("HOLE_NEXT", "HOLE_V0");
        let hole_b = module(&[uses, ATTRIBUTES]);
        assert_ne!(keys_of(&hole_a)["u"], keys_of(&hole_b)["u"]);
        assert_eq!(keys_of(&base)["r"], keys_of(&other)["r"]);
    }

    #[test]
    fn comments_strings_and_cyclic_metadata_do_not_break_keys() {
        let plain = module(&[&row("r", "h"), &helper("h", 7), ATTRIBUTES]);
        let commented = plain.replace("  %v = call", "  ; a comment with %x and @y\n  %v = call");
        assert_eq!(keys_of(&plain)["r"], keys_of(&commented)["r"]);
        let cyclic = format!("{plain}!3 = distinct !{{!3, !\"domain\"}}\n");
        let text = module(&[
            "define dso_local preserve_nonecc i64 @c(i64 %p0) unnamed_addr #1 {\n  %v = add i64 %p0, 1, !noalias !3\n  ret i64 %v\n}",
            "attributes #1 = { nounwind }",
            "!3 = distinct !{!3, !\"domain\"}",
        ]);
        assert!(keys_of(&text).contains_key("c"));
        assert!(keys_of(&cyclic).contains_key("r"));
    }

    #[test]
    fn a_type_definition_a_row_uses_is_part_of_its_key() {
        let uses = |ty: &str| {
            module(&[&format!("define dso_local i64 @t(ptr %p) unnamed_addr #1 {{\n  %v = load %\"S\", ptr %p\n  ret i64 0\n}}").replace("%\"S\"", "%\"core::S\""), &format!("%\"core::S\" = type {{ {ty} }}"), "attributes #1 = { nounwind }"])
        };
        assert_ne!(keys_of(&uses("i64"))["t"], keys_of(&uses("i32, i32"))["t"]);
    }

    #[test]
    fn name_normalization_numbers_locals_by_appearance() {
        let module = Module::default();
        let mut norm = Norm::new(&module, "");
        norm.run("%zeta = add i64 %alpha, %zeta\nbb9:\n  br label %bb9", true);
        let text: Vec<u8> = norm.finish().into_iter().flat_map(|seg| match seg {
            Seg::Text(bytes) => bytes,
            Seg::Item(_) => unreachable!("no item"),
        }).collect();
        assert_eq!(String::from_utf8(text).unwrap(), "%L0 = add i64 %L1, %L0\n%L2:\n  br label %L2");
    }
}
