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

/// The names a replay interns: the finished program's table, every one of which a replay of
/// one of its functions finds again.
pub(crate) struct ProgramAtomIndex {
    names: FxHashMap<std::rc::Rc<str>, Atom>,
}

impl ProgramAtomIndex {
    pub(crate) fn new(atoms: &crate::bytecode::AtomTable) -> Self {
        let mut names = FxHashMap::default();
        for atom in 0..atoms.len() {
            // The first atom spelled a name is the one interning finds.
            names
                .entry(std::rc::Rc::from(&atoms[atom]))
                .or_insert(atom as Atom);
        }
        Self { names }
    }

    pub(crate) fn find(&self, name: &str) -> Option<Atom> {
        self.names.get(name).copied()
    }
}

/// The functions a compilation makes, by id. At load the compiler owns every record. A replay
/// makes one record, the replayed function's; the functions nested in it keep the ids and
/// records the load gave them, and the replay reads their headers from the program.
pub(super) enum FunctionTable<'a> {
    Owned(Vec<Option<crate::bytecode::Function>>),
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
            Self::Owned(records) => records.len(),
            Self::Replay { next, .. } => *next as usize,
        }
    }

    pub(super) fn reserve_exact(&mut self, additional: usize) {
        if let Self::Owned(records) = self {
            records.reserve_exact(additional);
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
            Self::Owned(records) => records.push(None),
            Self::Replay { next, .. } => *next += 1,
        }
        id
    }

    pub(super) fn set(&mut self, id: u32, function: crate::bytecode::Function) {
        match self {
            Self::Owned(records) => records[id as usize] = Some(function),
            Self::Replay { target, record, .. } => {
                assert_eq!(id, *target, "a replay makes only its target's record");
                *record = Some(function);
            }
        }
    }

    /// A record this compilation made.
    pub(super) fn get(&self, id: u32) -> Option<&crate::bytecode::Function> {
        match self {
            Self::Owned(records) => records.get(id as usize)?.as_ref(),
            Self::Replay { target, record, .. } => (id == *target).then_some(record.as_ref()?),
        }
    }

    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Owned(records) => records.get_mut(id as usize)?.as_mut(),
            Self::Replay { target, record, .. } => {
                if id == *target { record.as_mut() } else { None }
            }
        }
    }

    /// The header of a function a replay skipped: the load made its record.
    pub(super) fn loaded_header(&self, id: u32) -> Option<&crate::bytecode::FunctionHeader> {
        match self {
            Self::Owned(_) => None,
            Self::Replay { program, .. } => Some(program.get(id as usize)?.header()),
        }
    }

    /// The records this compilation made.
    pub(super) fn records(&self) -> impl Iterator<Item = &crate::bytecode::Function> {
        let (owned, replayed) = match self {
            Self::Owned(records) => (Some(records.iter().flatten()), None),
            Self::Replay { record, .. } => (None, record.as_ref()),
        };
        owned.into_iter().flatten().chain(replayed)
    }

    pub(super) fn first_mut(&mut self) -> Option<&mut crate::bytecode::Function> {
        match self {
            Self::Owned(records) => records.first_mut()?.as_mut(),
            Self::Replay { .. } => None,
        }
    }

    /// Every record, in id order, once all are made.
    pub(super) fn into_records(self) -> Vec<crate::bytecode::Function> {
        match self {
            Self::Owned(records) => records
                .into_iter()
                .map(|record| record.expect("every reserved function is compiled"))
                .collect(),
            Self::Replay { record, .. } => record.into_iter().collect(),
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
    pub(super) scopes: Vec<std::rc::Rc<super::capture_scope::CaptureScope>>,
    pub(super) parent: u32,
    pub(super) with_depth: u16,
    pub(super) inherited_strict: bool,
}

/// Where an arrow function is made: everything its compilation reads beyond its own syntax,
/// which includes the `this`, `super` and home object of the code around it.
#[derive(Clone)]
pub(crate) struct ArrowContext {
    pub(super) scopes: Vec<std::rc::Rc<super::capture_scope::CaptureScope>>,
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

/// How a replayable function was compiled at load.
#[derive(Clone)]
pub(crate) enum ReplayCall {
    Plain(PlainFunctionContext),
    Arrow(ArrowContext),
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
    pub(super) atoms: std::cell::OnceCell<ProgramAtomIndex>,
    pub(super) constants: std::cell::OnceCell<FxHashMap<super::ConstantKey, u32>>,
    pub(super) method_sites: std::cell::OnceCell<Vec<super::MethodSiteSpec>>,
    pub(super) line_starts: std::cell::OnceCell<Vec<usize>>,
    pub(super) utf16_excess: std::cell::OnceCell<Vec<(usize, usize)>>,
}

/// How a program's functions can be compiled again, one at a time.
pub(crate) struct ProgramReplay {
    pub(super) source_type: oxc_span::SourceType,
    pub(super) root_strict: bool,
    pub(super) module_goal: bool,
    pub(super) specialized: bool,
    /// Where the load left the tables once each function was compiled.
    pub(super) exits: Box<[TableCursors]>,
    /// The functions a replay can compile, by id.
    pub(super) functions: FxHashMap<u32, FunctionReplay>,
    pub(super) views: ReplayViews,
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
        self.index.find(name)
    }

    fn table(&self) -> crate::bytecode::AtomTable {
        self.table.clone()
    }

    fn specialized(&self) -> bool {
        self.specialized
    }
}

/// The text around a function's source that gives it the syntactic context it had: a
/// function parses as an expression anywhere; an arrow keeps the `this`, `super` and
/// `new.target` of a constructor (strict code) or of a method (sloppy code, which has no
/// `super()` to keep).
fn replay_wrapper(call: &ReplayCall, strict: bool) -> (&'static str, &'static str) {
    match call {
        ReplayCall::Plain(_) => ("(", ")"),
        ReplayCall::Arrow(_) if strict => ("(class extends Object{constructor(){(", ")}})"),
        ReplayCall::Arrow(_) => ("({m(){(", ")}})"),
    }
}

/// Moves every span of a tree parsed from wrapped text by `delta`, so the function's own
/// spans are offsets into the program's text again.
struct ShiftSpans(u32);

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

/// Inverts `Compiler::flatten_method_sites`.
fn method_site_specs(program: &crate::bytecode::ResidualProgram) -> Vec<super::MethodSiteSpec> {
    program
        .method_sites
        .iter()
        .map(|site| {
            let start = site.argument_start as usize;
            let arguments = program.method_arguments[start..start + usize::from(site.argument_count)].to_vec();
            (site.atom, site.cache, arguments, site.receiver_path)
        })
        .collect()
}

/// A function of a finished program, compiled again, and the table entries it wrote.
#[cfg_attr(not(feature = "verify-replay"), allow(dead_code))]
pub(crate) struct Recompiled {
    pub(crate) function: crate::bytecode::Function,
    /// Each entry the replay wrote, rendered, beside the program's entry at its index.
    pub(crate) entries: Vec<(&'static str, usize, String, String)>,
}

/// Compiles function `id` of `program` again from its source, as the load compiled it.
#[cfg_attr(not(feature = "verify-replay"), allow(dead_code))]
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
    let span = header.source_span.ok_or("the function has no source span")?;
    let snippet = text
        .get(span.start as usize..span.end as usize)
        .ok_or("the source span lies outside the source")?;
    let strict = replay.module_goal
        || match &function_replay.call {
            ReplayCall::Plain(context) => context.inherited_strict,
            ReplayCall::Arrow(context) => context.inherited_strict,
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
    let delta = span.start.wrapping_sub(prefix.len() as u32);
    oxc_ast_visit::VisitMut::visit_program(&mut ShiftSpans(delta), &mut parsed.program);
    let views = &replay.views;
    let atom_index = views.atoms.get_or_init(|| ProgramAtomIndex::new(&program.atoms));
    let atoms = ReplayAtoms {
        table: &program.atoms,
        index: atom_index,
        specialized: replay.specialized,
    };
    let mode = if replay.specialized {
        SpecializationMode::Enabled
    } else {
        SpecializationMode::Disabled
    };
    let mut compiler = Compiler::new(&program.source_name, text, mode, &atoms, FxHashMap::default());
    compiler.source_line_starts = std::borrow::Cow::Borrowed(
        views
            .line_starts
            .get_or_init(|| crate::bytecode::source_line_starts(text).collect()),
    );
    compiler.source_utf16_excess = std::borrow::Cow::Borrowed(
        views
            .utf16_excess
            .get_or_init(|| super::source_utf16_excess(text)),
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
    compiler.constant_index = std::borrow::Cow::Borrowed(views.constants.get_or_init(|| {
        let mut index = FxHashMap::default();
        for (position, constant) in program.constants.iter().enumerate() {
            index
                .entry(super::ConstantKey::from(constant))
                .or_insert(position as u32);
        }
        index
    }));
    compiler.constants = SiteTable::replay(&program.constants, 0);
    compiler.method_sites =
        SiteTable::replay(views.method_sites.get_or_init(|| method_site_specs(program)), 0);
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
    compiler.seek(function_replay.entry);
    let made = match &function_replay.call {
        ReplayCall::Plain(context) => {
            let function = match single_expression(&parsed.program.body) {
                Some(oxc_ast::ast::Expression::FunctionExpression(function)) => function,
                _ => return Err("the wrapped source holds no function".into()),
            };
            compiler.compile_plain_function(function, context.clone())
        }
        ReplayCall::Arrow(context) => {
            let arrow = wrapped_arrow(&parsed.program).ok_or("the wrapped source holds no arrow")?;
            compiler.compile_replayable_arrow(arrow, context.clone())
        }
    };
    if made != id {
        return Err(format!("the replay compiled function {made}, not {id}"));
    }
    if let Some(error) = compiler.errors.first() {
        return Err(format!("the replay failed to compile: {}", error.message));
    }
    let mut function = std::mem::take(&mut compiler.functions)
        .into_records()
        .pop()
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
        compiler.method_sites.as_slice(),
        compiler.field_sites.as_slice(),
        compiler.superinstructions.as_slice(),
    );
    function.code.shrink_to_fit();
    function.wide.shrink_to_fit();
    function.source_positions.shrink_to_fit();
    // The function making this one named it after compiling it.
    function.inferred_name = header.inferred_name;
    let mut entries = Vec::new();
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
    written(&mut entries, "method_sites", &compiler.method_sites);
    written(&mut entries, "field_sites", &compiler.field_sites);
    written(&mut entries, "object_sites", &compiler.object_sites);
    written(&mut entries, "regexp_literal_sites", &compiler.regexp_literal_sites);
    written(&mut entries, "literal_templates", &compiler.literal_templates);
    written(&mut entries, "superinstructions", &compiler.superinstructions);
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
        let loaded = format!("{:?}", &*program.functions[id as usize]);
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
