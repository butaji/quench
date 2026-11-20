//! What a shard's rows reach in its unoptimized LLVM IR (`--emit=llvm-ir -C opt-level=0 -C no-prepopulate-passes`,
//! where every source function a row runs is still its own `define` and every use a `@name`): the identifiers of
//! every reachable definition's symbol. A row has no call (`checks`), so its object is made of the bodies of the
//! functions it reaches and nothing else of the crate; an edit inside the body of a function no row of the shard
//! reaches cannot change the shard's object (`edits`).
//!
//! The identifiers are read from each symbol as a superset: every `<length><identifier>` run a v0 symbol
//! (`_R...`) could hold, read at every digit, so a function's own name, its parents' and a closure's enclosing
//! function are all in it.

use std::collections::{BTreeSet, HashMap, VecDeque};

/// Whether `b` may be part of an unquoted LLVM name.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$' | b'-')
}

/// The name at the start of `text` (after its `@`): quoted or a run of name bytes.
fn read_name(text: &str) -> &str {
    let b = text.as_bytes();
    if b.first() == Some(&b'"') {
        let close = b[1..].iter().position(|&x| x == b'"').map_or(b.len(), |p| 1 + p);
        return &text[1..close];
    }
    let end = b.iter().take_while(|&&x| is_name_byte(x)).count();
    &text[..end]
}

/// Every `@name` a definition's text uses.
fn uses(body: &str) -> impl Iterator<Item = &str> {
    body.match_indices('@').map(move |(at, _)| read_name(&body[at + 1..])).filter(|name| !name.is_empty())
}

/// The definitions of an LLVM IR module by name: each function's text from `define` to its closing brace, and each
/// global's line.
fn definitions(ll: &str) -> HashMap<&str, &str> {
    let mut items = HashMap::new();
    let base = ll.as_ptr() as usize;
    let mut lines = ll.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("define ") {
            let start = line.as_ptr() as usize - base;
            let mut end = start + line.len();
            for next in lines.by_ref() {
                end = next.as_ptr() as usize - base + next.len();
                if next == "}" {
                    break;
                }
            }
            if let Some(at) = line.find('@') {
                items.insert(read_name(&line[at + 1..]), &ll[start..end]);
            }
        } else if let Some(rest) = line.strip_prefix('@') {
            items.insert(read_name(rest), line);
        }
    }
    items
}

/// Every identifier a v0 symbol could hold: at each run of digits `n` (one not preceded by a digit), the `n` bytes
/// after it where they are identifier bytes. A non-v0 name (a row's own) is its own identifier.
pub fn identifiers(symbol: &str, into: &mut BTreeSet<String>) {
    into.insert(symbol.to_string());
    let b = symbol.as_bytes();
    let mut at = 0;
    while at < b.len() {
        if b[at].is_ascii_digit() && (at == 0 || !b[at - 1].is_ascii_digit()) {
            let digits = b[at..].iter().take_while(|x| x.is_ascii_digit()).count();
            // A v0 identifier may be preceded by `_` to separate it from digits it starts with.
            let mut start = at + digits;
            if b.get(start) == Some(&b'_') {
                start += 1;
            }
            if let Ok(n) = symbol[at..at + digits].parse::<usize>() {
                for from in [at + digits, start] {
                    if n > 0 && from + n <= b.len() && b[from..from + n].iter().all(|&x| x.is_ascii_alphanumeric() || x == b'_') {
                        into.insert(symbol[from..from + n].to_string());
                    }
                }
            }
            at += digits;
        } else {
            at += 1;
        }
    }
}

/// The identifiers of every definition the rows (`is_row`) of the module `ll` reach through uses.
pub fn reached(ll: &str, is_row: impl Fn(&str) -> bool) -> BTreeSet<String> {
    let items = definitions(ll);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = items.keys().copied().filter(|name| is_row(name) && items[name].starts_with("define ")).collect();
    seen.extend(queue.iter().copied());
    while let Some(name) = queue.pop_front() {
        for used in uses(items[name]) {
            if items.contains_key(used) && seen.insert(used) {
                queue.push_back(used);
            }
        }
    }
    let mut out = BTreeSet::new();
    for name in seen {
        identifiers(name, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_follows_uses_and_reads_identifiers() {
        let ll = "define void @n_1() {\n  call void @_RNvNtCs1a_5crate7numeric4step()\n  ret void\n}\n\
                  define void @_RNvNtCs1a_5crate7numeric4step() {\n  ret void\n}\n\
                  define void @_RNvNtCs1a_5crate7compare8identity() {\n  ret void\n}\n";
        let ids = reached(ll, |name| !name.starts_with("_R"));
        assert!(ids.contains("step") && ids.contains("numeric") && ids.contains("n_1"));
        assert!(!ids.contains("identity"));
    }
}
