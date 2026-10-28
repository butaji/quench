//! Recompiling one function of a finished program from its source span.
//!
//! A program is compiled whole at load, so every fact one function's code depends on is
//! decided there: which of its locals its closures capture, the slots of the scopes it
//! closes over, and the index of every constant and site it adds to the program's tables.
//! A replay parses the function's text again and emits it against those decisions, adding
//! nothing to the program: each table entry the function makes already exists at the index
//! the replay reaches, and each function nested in it already has its id and its range.

use crate::bytecode::Atom;
use rustc_hash::FxHashMap;
use std::ops::{Index, IndexMut};

/// A program-wide table the compiler appends to. At load the compiler owns its entries. A
/// replay reads the finished program's entries and advances a cursor through those the
/// replayed function adds; an entry it writes is kept beside the program's, so a replay never
/// changes the table it reads.
pub(super) enum SiteTable<'a, T> {
    Owned(Vec<T>),
    Replay {
        entries: &'a [T],
        next: usize,
        written: FxHashMap<usize, T>,
    },
}

impl<T> Default for SiteTable<'_, T> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<'a, T: Clone> SiteTable<'a, T> {
    pub(super) fn replay(entries: &'a [T], next: usize) -> Self {
        Self::Replay {
            entries,
            next,
            written: FxHashMap::default(),
        }
    }

    /// The index the next entry takes.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Owned(entries) => entries.len(),
            Self::Replay { next, .. } => *next,
        }
    }

    pub(super) fn push(&mut self, value: T) {
        match self {
            Self::Owned(entries) => entries.push(value),
            Self::Replay { next, written, .. } => {
                written.insert(*next, value);
                *next += 1;
            }
        }
    }

    /// Moves a replay's cursor past entries a nested function added at load.
    pub(super) fn skip_to(&mut self, index: usize) {
        if let Self::Replay { next, .. } = self {
            *next = index;
        }
    }

    /// Every entry: at load, those made so far; in a replay, the finished program's.
    pub(super) fn as_slice(&self) -> &[T] {
        match self {
            Self::Owned(entries) => entries,
            Self::Replay { entries, .. } => entries,
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<&T> {
        match self {
            Self::Owned(entries) => entries.get(index),
            Self::Replay { entries, written, .. } => {
                written.get(&index).or_else(|| entries.get(index))
            }
        }
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.get(index)?;
        Some(&mut self[index])
    }

    /// The owned entries; a replay owns none.
    pub(super) fn into_vec(self) -> Vec<T> {
        match self {
            Self::Owned(entries) => entries,
            Self::Replay { .. } => Vec::new(),
        }
    }

    /// The entries a replay wrote, by index.
    #[cfg(feature = "verify-replay")]
    pub(super) fn written(&self) -> impl Iterator<Item = (usize, &T)> {
        match self {
            Self::Owned(_) => None,
            Self::Replay { written, .. } => Some(written.iter().map(|(index, value)| (*index, value))),
        }
        .into_iter()
        .flatten()
    }
}

impl<T: Clone> Index<usize> for SiteTable<'_, T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        self.get(index).expect("site index within its table")
    }
}

impl<T: Clone> IndexMut<usize> for SiteTable<'_, T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        match self {
            Self::Owned(entries) => &mut entries[index],
            Self::Replay { entries, written, .. } => written
                .entry(index)
                .or_insert_with(|| entries[index].clone()),
        }
    }
}

impl<T> From<Vec<T>> for SiteTable<'_, T> {
    fn from(entries: Vec<T>) -> Self {
        Self::Owned(entries)
    }
}

/// The next index of every table and counter a function's compilation advances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableCursors {
    pub(crate) functions: u32,
    pub(crate) constants: u32,
    pub(crate) cache_sites: u16,
    pub(crate) template_sites: u32,
    pub(crate) method_sites: u32,
    pub(crate) field_sites: u32,
    pub(crate) object_sites: u32,
    pub(crate) regexp_literal_sites: u32,
    pub(crate) literal_templates: u32,
}

/// The names a replay interns: the atoms the program's compilation interned, found by their
/// name's hash. A replay of one of its functions interns only names the load did.
pub(crate) struct ProgramAtomIndex {
    atoms: hashbrown::HashTable<Atom>,
}

fn atom_name_hash(name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    name.hash(&mut hasher);
    hasher.finish()
}

impl ProgramAtomIndex {
    pub(super) fn new(
        interned: impl Iterator<Item = Atom>,
        atoms: &crate::bytecode::AtomTable,
    ) -> Self {
        let mut interned = interned.collect::<Vec<_>>();
        // A name interned twice is found as its first atom.
        interned.sort_unstable();
        let mut index = hashbrown::HashTable::with_capacity(interned.len());
        for atom in interned {
            let name = &atoms[atom as usize];
            let hash = atom_name_hash(name);
            if index.find(hash, |&known: &Atom| &atoms[known as usize] == name).is_none() {
                index.insert_unique(hash, atom, |&known: &Atom| atom_name_hash(&atoms[known as usize]));
            }
        }
        Self { atoms: index }
    }

    pub(crate) fn find(&self, atoms: &crate::bytecode::AtomTable, name: &str) -> Option<Atom> {
        self.atoms.find(atom_name_hash(name), |&atom| &atoms[atom as usize] == name).copied()
    }
}

/// The index of each constant a compilation added, by value: candidates found by their
/// key's hash, then compared with the constants themselves, which are the one record of
/// each value. At load the compiler's own constants back it; a replay finds every constant
/// of the finished program.
pub(super) enum ConstantIndex<'a> {
    /// Each key hash's constant indices, in index order.
    Owned(FxHashMap<u64, smallvec::SmallVec<[u32; 1]>>),
    Replay {
        /// Each constant's key hash with its index, in hash then index order.
        hashes: &'a [(u64, u32)],
        constants: &'a [crate::bytecode::Constant],
    },
}

impl Default for ConstantIndex<'_> {
    fn default() -> Self {
        Self::Owned(FxHashMap::default())
    }
}

fn constant_key_hash(key: super::ConstantKey<'_>) -> u64 {
    use std::hash::BuildHasher as _;
    rustc_hash::FxBuildHasher.hash_one(key)
}

impl ConstantIndex<'_> {
    /// The first index of a constant whose key is `key`; `added` holds the constants the
    /// compiler added, which an owned index names.
    pub(super) fn get(
        &self,
        key: super::ConstantKey<'_>,
        added: &SiteTable<'_, crate::bytecode::Constant>,
    ) -> Option<u32> {
        let hash = constant_key_hash(key);
        let matches = |constant: &crate::bytecode::Constant| super::ConstantKey::from(constant) == key;
        match self {
            Self::Owned(index) => {
                index.get(&hash)?.iter().copied().find(|index| matches(&added[*index as usize]))
            }
            Self::Replay { hashes, constants } => {
                let start = hashes.partition_point(|(candidate, _)| *candidate < hash);
                hashes[start..]
                    .iter()
                    .take_while(|(candidate, _)| *candidate == hash)
                    .map(|(_, index)| *index)
                    .find(|index| matches(&constants[*index as usize]))
            }
        }
    }

    /// Records the first index of `key`. A replay adds no constant.
    pub(super) fn insert(&mut self, key: super::ConstantKey<'_>, index: u32) {
        match self {
            Self::Owned(entries) => entries.entry(constant_key_hash(key)).or_default().push(index),
            Self::Replay { .. } => {
                debug_assert!(false, "a replay adds no constant");
            }
        }
    }
}

/// `ConstantIndex::Replay`'s hashes of `constants`.
fn constant_hashes(constants: &[crate::bytecode::Constant]) -> Vec<(u64, u32)> {
    let mut hashes: Vec<(u64, u32)> = constants
        .iter()
        .enumerate()
        .map(|(index, constant)| (constant_key_hash(super::ConstantKey::from(constant)), index as u32))
        .collect();
    hashes.sort_unstable();
    hashes
}

/// One function of a compilation at load: reserved while it compiles, then compiled, or
/// finished and released when a replay can make its record again.
/// An entry is boxed when it holds anything: a large program's compilation keeps an entry
/// per function while most of them are released, and an inline record (408 bytes) left
/// each released entry that large (typescript.js: 21.7K entries, 8.9 MB at its compile's
/// peak).
pub(super) enum FunctionEntry {
    Reserved,
    Compiled(Box<crate::bytecode::Function>),
    Released(Box<ReleasedFunction>),
}

/// A function finished during its program's load and released: its header, which the
/// function making it may still name, and in verifying builds the record a replay must
/// equal.
pub(super) struct ReleasedFunction {
    pub(super) header: crate::bytecode::FunctionHeader,
    #[cfg(feature = "verify-replay")]
    pub(super) record: crate::bytecode::Function,
}

/// The functions a compilation makes, by id. At load the compiler owns every entry. A replay
/// makes one record, the replayed function's; the functions nested in it keep the ids and
/// records the load gave them, and the replay reads their headers from the program.
pub(super) enum FunctionTable<'a> {
    Owned(Vec<FunctionEntry>),
    Replay {
        program: &'a [crate::bytecode::FunctionSlot],
        target: u32,
        next: u32,
        record: Option<crate::bytecode::Function>,
    },
}

impl Default for FunctionTable<'_> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<'a> FunctionTable<'a> {
    /// The id the next function takes.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Owned(entries) => entries.len(),
            Self::Replay { next, .. } => *next as usize,
        }
    }

    pub(super) fn reserve_exact(&mut self, additional: usize) {
        if let Self::Owned(entries) = self {
            entries.reserve_exact(additional);
        }
    }

    /// Moves a replay past the ids of functions the load compiled.
    pub(super) fn seek(&mut self, id: u32) {
        if let Self::Replay { next, .. } = self {
            *next = id;
        }
    }

    /// Takes the next id for a function whose record follows.
    pub(super) fn reserve(&mut self) -> u32 {
        let id = self.len() as u32;
        match self {
            Self::Owned(entries) => entries.push(FunctionEntry::Reserved),
            Self::Replay { next, .. } => *next += 1,
        }
        id
    }

    pub(super) fn set(&mut self, id: u32, function: crate::bytecode::Function) {
        match self {
            Self::Owned(entries) => entries[id as usize] = FunctionEntry::Compiled(Box::new(function)),
            Self::Replay { target, record, .. } => {
                assert_eq!(id, *target, "a replay makes only its target's record");
                *record = Some(function);
            }
        }
    }

    /// A record this compilation made and still holds.
    pub(super) fn get(&self, id: u32) -> Option<&crate::bytecode::Function> {
        match self {
            Self::Owned(entries) => match entries.get(id as usize)? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            Self::Replay { target, record, .. } => (id == *target).then_some(record.as_ref()?),
        }
    }

    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Owned(entries) => match entries.get_mut(id as usize)? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            Self::Replay { target, record, .. } => {
                if id == *target { record.as_mut() } else { None }
            }
        }
    }

    /// The header of a function whose record this compilation does not hold: one a replay
    /// skipped, which the load made, or one the load released.
    pub(super) fn header_without_record(&self, id: u32) -> Option<&crate::bytecode::FunctionHeader> {
        match self {
            Self::Owned(entries) => match entries.get(id as usize)? {
                FunctionEntry::Released(released) => Some(&released.header),
                _ => None,
            },
            Self::Replay { program, .. } => Some(program.get(id as usize)?.header()),
        }
    }

    /// Names a released function made from an anonymous definition.
    pub(super) fn name_released(&mut self, id: u32, name: Atom) {
        if let Self::Owned(entries) = self
            && let Some(FunctionEntry::Released(released)) = entries.get_mut(id as usize)
        {
            released.header.inferred_name = Some(name);
            #[cfg(feature = "verify-replay")]
            {
                released.record.inferred_name = Some(name);
            }
        }
    }

    /// Takes the record of a compiled function to finish it.
    pub(super) fn take(&mut self, id: u32) -> Option<crate::bytecode::Function> {
        let Self::Owned(entries) = self else {
            return None;
        };
        let entry = entries.get_mut(id as usize)?;
        match std::mem::replace(entry, FunctionEntry::Reserved) {
            FunctionEntry::Compiled(function) => Some(*function),
            other => {
                *entry = other;
                None
            }
        }
    }

    /// Releases a finished function, keeping its header.
    pub(super) fn release(&mut self, id: u32, function: crate::bytecode::Function) {
        if let Self::Owned(entries) = self {
            entries[id as usize] = FunctionEntry::Released(Box::new(ReleasedFunction {
                header: crate::bytecode::FunctionHeader::of(&function),
                #[cfg(feature = "verify-replay")]
                record: function,
            }));
        }
    }

    pub(super) fn first_mut(&mut self) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Owned(entries) => match entries.first_mut()? {
                FunctionEntry::Compiled(function) => Some(function),
                _ => None,
            },
            Self::Replay { .. } => None,
        }
    }

    /// Every entry, in id order, once all are made.
    pub(super) fn into_entries(self) -> Vec<FunctionEntry> {
        match self {
            Self::Owned(entries) => entries,
            Self::Replay { record, .. } => record.into_iter().map(|record| FunctionEntry::Compiled(Box::new(record))).collect(),
        }
    }

    /// A replay's record.
    pub(super) fn into_replayed(self) -> Option<crate::bytecode::Function> {
        match self {
            Self::Owned(_) => None,
            Self::Replay { record, .. } => record,
        }
    }
}

/// Where a function declaration or expression is made: everything its compilation reads
/// beyond its own syntax.
#[derive(Clone)]
pub(crate) struct PlainFunctionContext {
    /// A function expression binds its own name inside its body.
    pub(super) expression: bool,
    /// `export default function () {}`, which is named `default`.
    pub(super) default_export: bool,
    pub(super) scopes: super::capture_scope::ScopeChain,
    pub(super) parent: u32,
    pub(super) with_depth: u16,
    pub(super) inherited_strict: bool,
    /// A function expression whose `prototype` no program text observes (`call_only`).
    pub(super) prototype_unobservable: bool,
}

/// Where an arrow function is made: everything its compilation reads beyond its own syntax,
/// which includes the `this`, `super` and home object of the code around it.
#[derive(Clone)]
pub(crate) struct ArrowContext {
    pub(super) scopes: super::capture_scope::ScopeChain,
    pub(super) parent: Option<u32>,
    pub(super) super_static: bool,
    pub(super) super_home: bool,
    pub(super) super_home_atom: Option<Atom>,
    pub(super) inherited_strict: bool,
    pub(super) class_initializer: bool,
    pub(super) lexical_this_atom: Option<Atom>,
    pub(super) super_call_binds_this: bool,
    pub(super) with_depth: u16,
}

/// Where an object literal's method is made: its property's span, the home object `super`
/// reads, and the scopes and strictness around it.
#[derive(Clone)]
pub(crate) struct ObjectMethodContext {
    pub(super) property_span: oxc_span::Span,
    pub(super) home_atom: Atom,
    pub(super) scopes: super::capture_scope::ScopeChain,
    pub(super) parent: u32,
    pub(super) with_depth: u16,
    pub(super) inherited_strict: bool,
}

/// Where a class's method, getter or setter (not its constructor) is made: the class
/// element's span, which includes `static`, and the home object of its class side.
#[derive(Clone)]
pub(crate) struct ClassMethodContext {
    pub(super) element_span: oxc_span::Span,
    pub(super) home_atom: Atom,
    pub(super) scopes: super::capture_scope::ScopeChain,
    pub(super) parent: Option<u32>,
    pub(super) with_depth: u16,
}

/// How a replayable function was compiled at load.
#[derive(Clone)]
pub(crate) enum ReplayCall {
    Plain(PlainFunctionContext),
    Arrow(ArrowContext),
    ObjectMethod(ObjectMethodContext),
    ClassMethod(ClassMethodContext),
}

impl ReplayCall {
    /// The source the function is parsed from again: its own text, or, for a class method,
    /// its class element's, which also says whether it is static.
    fn source_span(&self, function: Option<crate::bytecode::SourceSpan>) -> Option<(u32, u32)> {
        match self {
            Self::ClassMethod(context) => Some((context.element_span.start, context.element_span.end)),
            Self::ObjectMethod(context) => Some((context.property_span.start, context.property_span.end)),
            Self::Plain(_) | Self::Arrow(_) => function.map(|span| (span.start, span.end)),
        }
    }
}

/// What a replay of one function reads beyond the program.
pub(crate) struct FunctionReplay {
    /// The cursors the load compiled the function from.
    pub(super) entry: TableCursors,
    pub(super) call: ReplayCall,
    pub(super) layout: super::capture_layout::LayoutInputs,
    /// The `arguments` slot `Compiler::compile_function` decided, before layout: it depends on
    /// the code of the arrows nested in the function, which a replay does not compile.
    pub(super) arguments_slot: Option<u16>,
    /// The field-site and superinstruction cursors the function's rewrites started from.
    pub(super) rewrite_cursors: (u32, u32),
}

/// Lazily derived views of a finished program that its replays share.
#[derive(Default)]
pub(super) struct ReplayViews {
    pub(super) constants: std::cell::OnceCell<Vec<(u64, u32)>>,
    pub(super) line_starts: std::cell::OnceCell<Vec<usize>>,
    pub(super) utf16_excess: std::cell::OnceCell<Vec<(usize, usize)>>,
}

/// How a program's functions can be compiled again, one at a time.
pub(crate) struct ProgramReplay {
    pub(super) source_type: oxc_span::SourceType,
    pub(super) root_strict: bool,
    pub(super) module_goal: bool,
    /// The starts of the function bodies the load compiled from their source parsed again
    /// (`lazy_bodies`), in order: a replay stands each in, so the code around it compiles
    /// as the load compiled it.
    pub(super) lazy_bodies: Box<[u32]>,
    pub(super) specialized: bool,
    pub(super) atoms: ProgramAtomIndex,
    /// Where the load left the tables once each function was compiled.
    pub(super) exits: Box<[TableCursors]>,
    /// The functions a replay can compile, by id.
    pub(super) functions: FxHashMap<u32, FunctionReplay>,
    /// The cache sites the load shared by name (`Compiler::name_shared_cache_site`).
    pub(super) name_shared_sites: super::NameSharedSiteMap,
    /// In verifying builds, the records of functions the load released, which their replays
    /// must equal.
    #[cfg(feature = "verify-replay")]
    pub(super) released: FxHashMap<u32, crate::bytecode::Function>,
    pub(super) views: ReplayViews,
}

impl ProgramReplay {
    /// The ids of the functions a replay can compile.
    pub(crate) fn replayable(&self) -> impl Iterator<Item = &u32> {
        self.functions.keys()
    }
}

impl std::fmt::Debug for ProgramReplay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProgramReplay")
            .field("functions", &self.functions.len())
            .finish_non_exhaustive()
    }
}

/// The atoms of a finished program as the prefix of a replay, which adds none.
struct ReplayAtoms<'p> {
    table: &'p crate::bytecode::AtomTable,
    index: &'p ProgramAtomIndex,
    specialized: bool,
}

impl super::atoms::AtomPrefix for ReplayAtoms<'_> {
    fn len(&self) -> usize {
        self.table.len()
    }

    fn name(&self, atom: Atom) -> &str {
        &self.table[atom as usize]
    }

    fn find(&self, name: &str) -> Option<Atom> {
        self.index.find(self.table, name)
    }

    fn table(&self) -> crate::bytecode::AtomTable {
        self.table.clone()
    }

    fn specialized(&self) -> bool {
        self.specialized
    }
}

/// The text around a function's source that gives it the syntactic context it had. A function
/// declaration parses again as one, and an expression as one: their names follow different
/// rules (`function* yield() {}` declares a generator only). A method's computed key belongs
/// to the code around it, which may be a generator or async function, so a method parses inside
/// an async generator, whose `yield` and `await` its own body does not see. An arrow keeps the
/// `this`, `super` and `new.target` of a constructor (strict code) or of a method (sloppy
/// code, which has no `super()` to keep).
fn replay_wrapper(call: &ReplayCall, strict: bool) -> (&'static str, &'static str) {
    match call {
        ReplayCall::Plain(context) => plain_function_wrapper(context),
        ReplayCall::ObjectMethod(_) => ("(async function*(){({", "})})"),
        ReplayCall::ClassMethod(_) => ("(async function*(){(class{", "})})"),
        ReplayCall::Arrow(_) if strict => ("(class extends Object{constructor(){(", ")}})"),
        ReplayCall::Arrow(_) => ("({m(){(", ")}})"),
    }
}

/// The text around a function declaration's or expression's source that parses it again as
/// one (`replay_wrapper`).
pub(super) fn plain_function_wrapper(context: &PlainFunctionContext) -> (&'static str, &'static str) {
    if context.expression {
        ("(", ")")
    } else if context.default_export {
        ("export default ", "")
    } else {
        ("", "")
    }
}

/// Moves every span of a tree parsed from wrapped text by `delta`, so the function's own
/// spans are offsets into the program's text again.
pub(super) struct ShiftSpans(pub(super) u32);

impl<'a> oxc_ast_visit::VisitMut<'a> for ShiftSpans {
    fn visit_span(&mut self, span: &mut oxc_span::Span) {
        span.start = span.start.wrapping_add(self.0);
        span.end = span.end.wrapping_add(self.0);
    }
}

fn parenthesized<'s, 'a>(
    expression: &'s oxc_ast::ast::Expression<'a>,
) -> Option<&'s oxc_ast::ast::Expression<'a>> {
    match expression {
        oxc_ast::ast::Expression::ParenthesizedExpression(inner) => Some(&inner.expression),
        _ => None,
    }
}

fn single_expression<'s, 'a>(
    statements: &'s [oxc_ast::ast::Statement<'a>],
) -> Option<&'s oxc_ast::ast::Expression<'a>> {
    match statements {
        [oxc_ast::ast::Statement::ExpressionStatement(statement)] => {
            parenthesized(&statement.expression)
        }
        _ => None,
    }
}

/// The function `replay_wrapper` placed a plain function's source as.
fn wrapped_function<'s, 'a>(
    program: &'s oxc_ast::ast::Program<'a>,
) -> Option<&'s oxc_ast::ast::Function<'a>> {
    match program.body.as_slice() {
        [oxc_ast::ast::Statement::FunctionDeclaration(function)] => Some(function),
        [oxc_ast::ast::Statement::ExportDefaultDeclaration(export)] => match &export.declaration {
            oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                Some(function)
            }
            _ => None,
        },
        body => match single_expression(body)? {
            oxc_ast::ast::Expression::FunctionExpression(function) => Some(function),
            _ => None,
        },
    }
}

/// The class or object `replay_wrapper` placed a method's source in, inside an async
/// generator.
fn wrapped_method_holder<'s, 'a>(
    program: &'s oxc_ast::ast::Program<'a>,
) -> Option<&'s oxc_ast::ast::Expression<'a>> {
    match single_expression(&program.body)? {
        oxc_ast::ast::Expression::FunctionExpression(holder) => {
            single_expression(&holder.body.as_ref()?.statements)
        }
        _ => None,
    }
}

/// The arrow `replay_wrapper` placed in the body of a constructor or method.
fn wrapped_arrow<'s, 'a>(
    program: &'s oxc_ast::ast::Program<'a>,
) -> Option<&'s oxc_ast::ast::ArrowFunctionExpression<'a>> {
    let method = match single_expression(&program.body)? {
        oxc_ast::ast::Expression::ClassExpression(class) => match class.body.body.as_slice() {
            [oxc_ast::ast::ClassElement::MethodDefinition(method)] => &method.value,
            _ => return None,
        },
        oxc_ast::ast::Expression::ObjectExpression(object) => match object.properties.as_slice() {
            [oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property)] => {
                match &property.value {
                    oxc_ast::ast::Expression::FunctionExpression(function) => function,
                    _ => return None,
                }
            }
            _ => return None,
        },
        _ => return None,
    };
    match single_expression(&method.body.as_ref()?.statements)? {
        oxc_ast::ast::Expression::ArrowFunctionExpression(arrow) => Some(arrow),
        _ => None,
    }
}

/// A function of a finished program, compiled again, and the table entries it wrote.
#[cfg_attr(not(feature = "verify-replay"), allow(dead_code))]
pub(crate) struct Recompiled {
    pub(crate) function: crate::bytecode::Function,
    /// Each entry the replay wrote, rendered, beside the program's entry at its index.
    pub(crate) entries: Vec<(&'static str, usize, String, String)>,
}

/// Compiles function `id` of `program` again from its source, as the load compiled it.
pub(crate) fn recompile(
    program: &crate::bytecode::ResidualProgram,
    id: u32,
) -> Result<Recompiled, String> {
    use super::{Compiler, Constant, SpecializationMode, capture_layout, liveness};
    let replay = program.replay.as_deref().ok_or("the program keeps no replay")?;
    let function_replay = replay
        .functions
        .get(&id)
        .ok_or("the function is not replayable")?;
    let source = program.source.as_ref().ok_or("the program keeps no source")?;
    if source.base != 0 {
        return Err("the program keeps part of its compiled source".into());
    }
    let text: &str = &source.text;
    let header = *program
        .functions
        .get(id as usize)
        .ok_or("no such function")?
        .header();
    let (start, end) = function_replay
        .call
        .source_span(header.source_span)
        .ok_or("the function has no source span")?;
    let snippet = text
        .get(start as usize..end as usize)
        .ok_or("the source span lies outside the source")?;
    let strict = replay.module_goal
        || match &function_replay.call {
            ReplayCall::Plain(context) => context.inherited_strict,
            ReplayCall::Arrow(context) => context.inherited_strict,
            ReplayCall::ObjectMethod(context) => context.inherited_strict,
            ReplayCall::ClassMethod(_) => true,
        };
    let (prefix, suffix) = replay_wrapper(&function_replay.call, strict);
    let wrapped = format!("{prefix}{snippet}{suffix}");
    let allocator = oxc_allocator::Allocator::with_capacity(wrapped.len().saturating_mul(6));
    let mut parsed = super::annex_b_targets::parse_program(&allocator, &wrapped, replay.source_type);
    if parsed.stack_exhausted || !parsed.diagnostics.is_empty() {
        return Err(format!(
            "the function's source does not parse alone: {}",
            parsed
                .diagnostics
                .first()
                .map_or_else(String::new, ToString::to_string)
        ));
    }
    let delta = start.wrapping_sub(prefix.len() as u32);
    oxc_ast_visit::VisitMut::visit_program(&mut ShiftSpans(delta), &mut parsed.program);
    // The bodies in the function's source the load compiled from their source parsed again,
    // its own included.
    let lazy_bodies: rustc_hash::FxHashSet<u32> = {
        let from = replay.lazy_bodies.partition_point(|body| *body < start);
        let to = replay.lazy_bodies.partition_point(|body| *body < end);
        replay.lazy_bodies[from..to].iter().copied().collect()
    };
    super::lazy_bodies::stand_in(&allocator, &mut parsed.program, &lazy_bodies);
    let views = &replay.views;
    let atoms = ReplayAtoms {
        table: &program.atoms,
        index: &replay.atoms,
        specialized: replay.specialized,
    };
    let mode = if replay.specialized {
        SpecializationMode::Enabled
    } else {
        SpecializationMode::Disabled
    };
    let mut compiler = Compiler::with_layout(
        &program.source_name,
        text,
        mode,
        &atoms,
        FxHashMap::default(),
        std::borrow::Cow::Borrowed(
            views
                .line_starts
                .get_or_init(|| crate::bytecode::source_line_starts(text).collect()),
        ),
        std::borrow::Cow::Borrowed(
            views
                .utf16_excess
                .get_or_init(|| super::source_utf16_excess(text)),
        ),
    );
    compiler.source_comments = parsed
        .program
        .comments
        .iter()
        .map(|comment| {
            let mut span = comment.span;
            ShiftSpans(delta).shift(&mut span);
            span
        })
        .collect();
    compiler.source_type = replay.source_type;
    compiler.root_strict = replay.root_strict;
    compiler.module_goal = replay.module_goal;
    compiler.constant_index = ConstantIndex::Replay {
        hashes: views
            .constants
            .get_or_init(|| constant_hashes(&program.constants)),
        constants: &program.constants,
    };
    compiler.constants = SiteTable::replay(&program.constants, 0);
    // A replay reads method sites from the program (`Compiler::method_arguments`).
    compiler.method_sites = SiteTable::replay(&[], 0);
    compiler.replay_program = Some(program);
    compiler.field_sites = SiteTable::replay(&program.field_sites, 0);
    compiler.object_sites = SiteTable::replay(&program.object_sites, 0);
    compiler.regexp_literal_sites = SiteTable::replay(&program.regexp_literal_sites, 0);
    compiler.literal_templates = SiteTable::replay(&program.literal_templates, 0);
    compiler.superinstructions = SiteTable::replay(&program.superinstructions, 0);
    compiler.functions = FunctionTable::Replay {
        program: &program.functions,
        target: id,
        next: 0,
        record: None,
    };
    compiler.replay_target = Some((id, &replay.exits));
    compiler.lazy_bodies = lazy_bodies;
    compiler.name_shared_sites = super::NameSharedSites::Replay(&replay.name_shared_sites);
    compiler.seek(function_replay.entry);
    let made = match &function_replay.call {
        ReplayCall::Plain(context) => {
            let function = wrapped_function(&parsed.program)
                .ok_or("the wrapped source holds no function")?;
            compiler.compile_plain_function(function, context.clone())
        }
        ReplayCall::Arrow(context) => {
            let arrow = wrapped_arrow(&parsed.program).ok_or("the wrapped source holds no arrow")?;
            compiler.compile_replayable_arrow(arrow, context.clone())
        }
        ReplayCall::ObjectMethod(context) => {
            let function = match wrapped_method_holder(&parsed.program) {
                Some(oxc_ast::ast::Expression::ObjectExpression(object)) => {
                    match object.properties.as_slice() {
                        [oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property)] => {
                            match &property.value {
                                oxc_ast::ast::Expression::FunctionExpression(function) => function,
                                _ => return Err("the wrapped property holds no method".into()),
                            }
                        }
                        _ => return Err("the wrapped object holds no property".into()),
                    }
                }
                _ => return Err("the wrapped source holds no object".into()),
            };
            compiler.compile_replayable_object_method(function, context.clone())
        }
        ReplayCall::ClassMethod(context) => {
            let method = match wrapped_method_holder(&parsed.program) {
                Some(oxc_ast::ast::Expression::ClassExpression(class)) => match class.body.body.as_slice() {
                    [oxc_ast::ast::ClassElement::MethodDefinition(method)] => method,
                    _ => return Err("the wrapped class holds no method".into()),
                },
                _ => return Err("the wrapped source holds no class".into()),
            };
            compiler.compile_replayable_class_method(method, context.clone())
        }
    };
    if made != id {
        return Err(format!("the replay compiled function {made}, not {id}"));
    }
    if let Some(error) = compiler.errors.first() {
        return Err(format!("the replay failed to compile: {}", error.message));
    }
    let mut function = std::mem::take(&mut compiler.functions)
        .into_replayed()
        .ok_or("the replay made no record")?;
    function.arguments_slot = function_replay.arguments_slot;
    capture_layout::apply_function(&mut function, id as usize, &function_replay.layout, &compiler.atoms);
    compiler
        .field_sites
        .skip_to(function_replay.rewrite_cursors.0 as usize);
    compiler
        .superinstructions
        .skip_to(function_replay.rewrite_cursors.1 as usize);
    compiler.rewrite_function(&mut function);
    let undefined_constant = compiler.constant(Constant::Undefined);
    compiler.lower_function(&mut function, undefined_constant);
    liveness::mark_register_roots(
        std::iter::once(&mut function),
        compiler.method_arguments(),
        compiler.field_sites.as_slice(),
        compiler.superinstructions.as_slice(),
    );
    function.shrink_to_fit();
    // The function making this one named it after compiling it.
    function.inferred_name = header.inferred_name;
    #[cfg_attr(not(feature = "verify-replay"), allow(unused_mut))]
    let mut entries = Vec::new();
    #[cfg(feature = "verify-replay")]
    {
    fn written<T: Clone + std::fmt::Debug>(
        entries: &mut Vec<(&'static str, usize, String, String)>,
        table: &'static str,
        replayed: &SiteTable<'_, T>,
    ) {
        for (index, value) in replayed.written() {
            let loaded = replayed
                .as_slice()
                .get(index)
                .map_or_else(|| "<none>".to_owned(), |loaded| format!("{loaded:?}"));
            entries.push((table, index, format!("{value:?}"), loaded));
        }
    }
    written(&mut entries, "constants", &compiler.constants);
    // A replay keeps no method-site table of its own; the program's are flattened.
    for (index, value) in compiler.method_sites.written() {
        let loaded = program.method_sites.get(index).map_or_else(
            || "<none>".to_owned(),
            |site| {
                let start = site.argument_start as usize;
                let arguments =
                    program.method_arguments[start..start + usize::from(site.argument_count)].to_vec();
                format!("{:?}", (site.atom, site.cache, arguments, site.receiver_path))
            },
        );
        entries.push(("method_sites", index, format!("{value:?}"), loaded));
    }
    written(&mut entries, "field_sites", &compiler.field_sites);
    written(&mut entries, "object_sites", &compiler.object_sites);
    written(&mut entries, "regexp_literal_sites", &compiler.regexp_literal_sites);
    written(&mut entries, "literal_templates", &compiler.literal_templates);
    written(&mut entries, "superinstructions", &compiler.superinstructions);
    }
    Ok(Recompiled { function, entries })
}

impl ShiftSpans {
    fn shift(&self, span: &mut oxc_span::Span) {
        oxc_ast_visit::VisitMut::visit_span(&mut Self(self.0), span);
    }
}

/// Recompiles every replayable function of `program` and appends to the file named by
/// `QUENCH_REPLAY_LOG` one line per function whose record or table entries differ from the
/// load's, or that does not recompile, and one summary line.
#[cfg(feature = "verify-replay")]
pub(crate) fn verify(program: &crate::bytecode::ResidualProgram) {
    use std::io::Write as _;
    let Some(replay) = program.replay.as_deref() else {
        return;
    };
    let Some(path) = std::env::var_os("QUENCH_REPLAY_LOG") else {
        return;
    };
    let mut ids: Vec<u32> = replay.functions.keys().copied().collect();
    ids.sort_unstable();
    let mut lines = Vec::new();
    let (mut same, mut differ, mut failed) = (0usize, 0usize, 0usize);
    for id in ids {
        let loaded = match program.functions[id as usize].record() {
            Some(record) => format!("{record:?}"),
            None => format!("{:?}", replay.released[&id]),
        };
        match recompile(program, id) {
            Ok(recompiled) => {
                let replayed = format!("{:?}", recompiled.function);
                let entry = recompiled
                    .entries
                    .iter()
                    .find(|(_, _, replayed, loaded)| replayed != loaded);
                if replayed == loaded && entry.is_none() {
                    same += 1;
                    continue;
                }
                differ += 1;
                let at = replayed
                    .bytes()
                    .zip(loaded.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(replayed.len().min(loaded.len()));
                let window = |text: &str| {
                    let start = at.saturating_sub(120);
                    text.get(start..(at + 240).min(text.len())).unwrap_or("").to_owned()
                };
                lines.push(format!(
                    "{{\"kind\":\"replay-differs\",\"program\":{:?},\"function\":{id},\"replayed\":{:?},\"loaded\":{:?},\"entry\":{:?}}}",
                    program.source_name,
                    window(&replayed),
                    window(&loaded),
                    entry
                ));
            }
            Err(error) => {
                failed += 1;
                lines.push(format!(
                    "{{\"kind\":\"replay-failed\",\"program\":{:?},\"function\":{id},\"error\":{:?}}}",
                    program.source_name, error
                ));
            }
        }
    }
    lines.push(format!(
        "{{\"kind\":\"replay-summary\",\"program\":{:?},\"functions\":{},\"same\":{same},\"differ\":{differ},\"failed\":{failed}}}",
        program.source_name,
        program.functions.len()
    ));
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all((lines.join("\n") + "\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    /// An entry costs a pointer plus its tag however large a record is: a released entry,
    /// which most of a large program's are while it compiles, holds nothing else.
    #[test]
    fn function_entries_hold_records_out_of_line() {
        assert_eq!(size_of::<super::FunctionEntry>(), 2 * size_of::<usize>());
    }
}
