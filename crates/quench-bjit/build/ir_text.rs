//! Reads the LLVM IR rustc emits for a stencil shard (`--emit=llvm-ir -C opt-level=0 -C no-prepopulate-passes`)
//! into its definitions, and normalizes a definition's text for `ir_hash`: every name that does not come from the
//! definition's content is replaced by its content's key, so two shards' copies of one definition read alike.

use std::collections::HashMap;

/// What a name refers to in the module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Function,
    Global,
    Type,
}

/// A definition of the module: its name (unquoted) and the text that normalizes into its key. A function's text is
/// the whole `define` through its closing brace; a global's and a type's is what follows `=`.
pub(crate) struct Item<'a> {
    pub(crate) name: &'a str,
    pub(crate) kind: Kind,
    pub(crate) body: &'a str,
}

/// The module's definitions, attribute groups and metadata, each a slice of the IR text.
#[derive(Default)]
pub(crate) struct Module<'a> {
    pub(crate) items: Vec<Item<'a>>,
    /// Functions and globals the module defines, by name.
    pub(crate) globals: HashMap<&'a str, usize>,
    pub(crate) types: HashMap<&'a str, usize>,
    /// `#N` to the group's text after `=`.
    pub(crate) attributes: HashMap<&'a str, &'a str>,
    /// `!N` to the node's text after `=`.
    pub(crate) metadata: HashMap<&'a str, &'a str>,
    /// Module-level lines that are neither definitions nor declarations (target, named metadata).
    pub(crate) prelude: Vec<&'a str>,
}

impl<'a> Module<'a> {
    pub(crate) fn add(&mut self, kind: Kind, name: &'a str, body: &'a str) -> usize {
        let id = self.items.len();
        self.items.push(Item { name, kind, body });
        match kind {
            Kind::Type => self.types.insert(name, id),
            Kind::Function | Kind::Global => self.globals.insert(name, id),
        };
        id
    }
}

/// A piece of a normalized item: literal text, or the key of another item.
pub(crate) enum Seg {
    Text(Vec<u8>),
    Item(usize),
}

/// Parses the module's definitions. Lines the format does not define (`ModuleID`, `source_filename`, `declare`) are
/// dropped; a global the module declares `external` is not a definition.
pub(crate) fn parse(src: &str) -> Module<'_> {
    let base = src.as_ptr() as usize;
    let offset = |line: &str| line.as_ptr() as usize - base;
    let mut module = Module::default();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let start = offset(line);
        if line.starts_with("define ") {
            let mut end = start + line.len();
            for next in lines.by_ref() {
                end = offset(next) + next.len();
                if next == "}" {
                    break;
                }
            }
            let name = name_after(line, '@');
            module.add(Kind::Function, name, &src[start..end]);
        } else if let Some(rest) = line.strip_prefix('@') {
            let (name, end) = read_name(rest, 0);
            let body = rest[end..].strip_prefix(" = ").unwrap_or("");
            if !body.starts_with("external ") {
                module.add(Kind::Global, name, body);
            }
        } else if let Some(rest) = line.strip_prefix('%') {
            let (name, end) = read_name(rest, 0);
            let body = rest[end..].strip_prefix(" = ").unwrap_or_else(|| panic!("a type line: {line}"));
            module.add(Kind::Type, name, body);
        } else if let Some(rest) = line.strip_prefix("attributes #") {
            let (id, text) = rest.split_once(" = ").expect("an attribute group");
            module.attributes.insert(id_of(id), text);
        } else if let Some(rest) = numbered_metadata(line) {
            let (id, text) = rest.split_once(" = ").expect("a metadata node");
            module.metadata.insert(id_of(id), text);
        } else if line.trim().is_empty() || line.starts_with(';') || line.starts_with("declare ") || line.starts_with("source_filename") {
            continue;
        } else {
            module.prelude.push(line);
        }
    }
    module
}

/// The digits of a metadata definition's `!N` head, the rest after the `!`.
fn numbered_metadata(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('!')?;
    rest.as_bytes().first().filter(|b| b.is_ascii_digit()).map(|_| rest)
}

/// The id of an attribute group or metadata node: the digits after its sigil, up to the first non-digit.
fn id_of(text: &str) -> &str {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    &text[..digits]
}

/// The name a `sigil` introduces in `line` (its first occurrence).
fn name_after(line: &str, sigil: char) -> &str {
    let at = line.find(sigil).unwrap_or_else(|| panic!("a name in {line}"));
    read_name(&line[at + 1..], 0).0
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$' | b'-')
}

/// The name at `at` in `text`: quoted (`"..."`, the name is the text between the quotes) or a run of name bytes.
/// Returns the name and the index just past it (past the closing quote).
fn read_name(text: &str, at: usize) -> (&str, usize) {
    let b = text.as_bytes();
    if b.get(at) == Some(&b'"') {
        let close = b[at + 1..].iter().position(|&x| x == b'"').map_or(b.len(), |p| at + 1 + p);
        return (&text[at + 1..close], (close + 1).min(b.len()));
    }
    let end = at + b[at..].iter().take_while(|&&x| is_name_byte(x)).count();
    (&text[at..end], end)
}

/// Normalizes one item's text (or a prelude line) into segments.
pub(crate) struct Norm<'m, 'a> {
    module: &'m Module<'a>,
    own: &'a str,
    locals: HashMap<&'a str, usize>,
    segs: Vec<Seg>,
    text: Vec<u8>,
    open_metadata: Vec<&'a str>,
}

impl<'m, 'a> Norm<'m, 'a> {
    pub(crate) fn new(module: &'m Module<'a>, own: &'a str) -> Self {
        Self { module, own, locals: HashMap::new(), segs: Vec::new(), text: Vec::new(), open_metadata: Vec::new() }
    }

    pub(crate) fn emit(&mut self, bytes: &[u8]) {
        self.text.extend_from_slice(bytes);
    }

    fn item(&mut self, id: usize) {
        if !self.text.is_empty() {
            self.segs.push(Seg::Text(std::mem::take(&mut self.text)));
        }
        self.segs.push(Seg::Item(id));
    }

    pub(crate) fn finish(mut self) -> Vec<Seg> {
        if !self.text.is_empty() {
            self.segs.push(Seg::Text(self.text));
        }
        self.segs
    }

    /// The number of a local, by first appearance; a value and a block share one namespace, as in LLVM.
    fn local(&mut self, name: &'a str) -> usize {
        let next = self.locals.len();
        *self.locals.entry(name).or_insert(next)
    }

    pub(crate) fn run(&mut self, text: &'a str, mut line_start: bool) {
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if line_start && is_name_byte(c) {
                let end = i + b[i..].iter().take_while(|&&x| is_name_byte(x)).count();
                if b.get(end) == Some(&b':') {
                    let k = self.local(&text[i..end]);
                    self.emit(format!("%L{k}:").as_bytes());
                    i = end + 1;
                    line_start = false;
                    continue;
                }
            }
            if line_start {
                // A line that is only a comment vanishes with its newline.
                let blanks = b[i..].iter().take_while(|&&x| x == b' ' || x == b'\t').count();
                if b.get(i + blanks) == Some(&b';') {
                    i = b[i + blanks..].iter().position(|&x| x == b'\n').map_or(b.len(), |p| i + blanks + p + 1);
                    continue;
                }
            }
            line_start = c == b'\n';
            match c {
                b';' => {
                    // A trailing comment: its alignment blanks are not part of the text.
                    while matches!(self.text.last(), Some(b' ' | b'\t')) {
                        self.text.pop();
                    }
                    i += b[i..].iter().position(|&x| x == b'\n').unwrap_or(b.len() - i);
                }
                b'"' => {
                    let end = b[i + 1..].iter().position(|&x| x == b'"').map_or(b.len(), |p| i + p + 2);
                    self.emit(&b[i..end]);
                    i = end;
                }
                b'@' | b'%' => {
                    let (name, end) = read_name(text, i + 1);
                    if name.is_empty() {
                        self.emit(&b[i..=i]);
                        i += 1;
                        continue;
                    }
                    if c == b'@' {
                        self.global(name, &text[i..end]);
                    } else {
                        self.percent(name);
                    }
                    i = end;
                }
                b'#' | b'!' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                    let end = i + 1 + b[i + 1..].iter().take_while(|x| x.is_ascii_digit()).count();
                    let id = &text[i + 1..end];
                    if c == b'#' {
                        match self.module.attributes.get(id).copied() {
                            Some(group) => self.run(group, false),
                            None => self.emit(&b[i..end]),
                        }
                    } else {
                        self.metadata(id, &b[i..end]);
                    }
                    i = end;
                }
                _ => {
                    self.emit(&b[i..=i]);
                    i += 1;
                }
            }
        }
    }

    fn global(&mut self, name: &str, raw: &str) {
        match self.module.globals.get(name).copied() {
            Some(_) if name == self.own => self.emit(b"@self"),
            Some(id) => self.item(id),
            None => self.emit(raw.as_bytes()),
        }
    }

    fn percent(&mut self, name: &'a str) {
        match self.module.types.get(name).copied() {
            Some(id) => self.item(id),
            None => {
                let k = self.local(name);
                self.emit(format!("%L{k}").as_bytes());
            }
        }
    }

    /// A metadata reference is its node's text, inlined; a node that refers to itself is cut at the cycle.
    fn metadata(&mut self, id: &'a str, raw: &[u8]) {
        let Some(&text) = self.module.metadata.get(id) else {
            return self.emit(raw);
        };
        if self.open_metadata.contains(&id) {
            return self.emit(b"!cycle");
        }
        self.open_metadata.push(id);
        self.run(text, false);
        self.open_metadata.pop();
    }
}

/// The segments of one item's definition.
pub(crate) fn normalize<'a>(module: &Module<'a>, item: &Item<'a>) -> Vec<Seg> {
    let mut norm = Norm::new(module, item.name);
    norm.run(item.body, true);
    norm.finish()
}

/// The module's prelude, normalized. It names no definition.
pub(crate) fn prelude_key_text(module: &Module<'_>) -> Vec<Seg> {
    let mut norm = Norm::new(module, "");
    for &line in &module.prelude {
        norm.run(line, true);
        norm.emit(b"\n");
    }
    norm.finish()
}
