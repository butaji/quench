//! A first-call chunk compiled into the program of its stub, after the program's own
//! functions and table entries, instead of as a program of its own: a call between the chunk's
//! functions and the program's is a call within one program (`ChunkDelta`).
//!
//! The chunk compiles against tables that begin at the program's lengths (`SiteTable::Delta`,
//! `FunctionTable::Chunk`), so every index it emits is the index its entry takes once the
//! program appends it, and reads the program's entries below them. Its function's parent is
//! the stub's, and a nested stub's free names resolve through the scopes the stub kept
//! (`FirstCallScopes`). The finishing passes run over the chunk's functions alone: the
//! program's were laid out at its load, and the chunk's captures of them are those the stub's
//! free names predicted. A chunk whose names resolve dynamically, or that makes a private name
//! or a tagged template, whose identities the program numbers as a whole, compiles as a
//! program of its own (`Vm::call_first_chunk`).
use super::*;
use crate::bytecode::{ChunkDelta, TableLengths};
use replay::{ConstantIndex, FunctionRecords, FunctionTable, constant_hashes, wrapped_arrow_function, wrapped_function};

impl Engine {
    /// The chunk of stub `function` of `program`, compiled to follow `program`'s tables, or
    /// `None` when it compiles as a program of its own.
    pub(crate) fn append_first_call_chunk<'a>(
        program: &'a ResidualProgram,
        function: u32,
        atom_prefix: &'a dyn AtomPrefix,
    ) -> Option<ChunkDelta> {
        let source = program.source.as_ref().filter(|source| source.base == 0)?;
        let replay = program.replay.as_deref()?;
        let header = *program.functions.get(function as usize)?.header();
        let span = header.source_span?;
        let chunk = ChunkText::of(source, span)?;
        let (line_starts, utf16_excess) = replay.views.layout(&source.text);
        let stubs = &program.first_call_scopes;
        let outer = stubs.binary_search_by_key(&function, |(stub, _)| *stub).ok().map(|at| (span.start, &stubs[at].1));
        let window = ChunkWindow { source, start: chunk.start, outer, line_starts, utf16_excess };
        let allocator = lazy_bodies::sized_arena(&chunk.text);
        let mut free_names = FreeNames::default();
        let nested = replay.direct_stubs;
        let mut parsed = parse_text(&allocator, &window, &chunk, span.start, nested.then_some(&mut free_names));
        let name = program.source_name.as_str();
        let mut prototype_unobservable = chunk_analysis(&parsed, name, nested.then_some((&chunk.text, usize::from(!header.is_arrow))))?;
        let mut compiler = window.compiler(name, SpecializationMode::of(atom_prefix), atom_prefix, &mut parsed.program, Default::default(), &mut Default::default(), &mut prototype_unobservable);
        compiler.prototype_unobservable = prototype_unobservable;
        compiler.source_comments = parsed.program.comments.iter().map(|comment| comment.span).collect();
        compiler.source_type = SourceType::script();
        compiler.first_call.free_names = free_names;
        compiler.first_call.nested_allowed = nested;
        compiler.first_call.direct = nested;
        compiler.follow(program, replay);
        let base = program.table_lengths();
        let target = if header.is_arrow {
            compiler.compile_chunk_arrow(wrapped_arrow_function(&parsed.program)?, header.parent?)
        } else {
            compiler.compile_chunk_function(wrapped_function(&parsed.program)?, header.parent?)
        };
        (target == base.functions as u32).then_some(())?;
        compiler.finish_chunk(base)
    }
}

/// The chunk's `text`, a function's source at `function_start` in the program's, parsed: whole,
/// or with the bodies inside the function skimmed where they may compile on first call too
/// (`free_names`, given for them).
fn parse_text<'a>(
    allocator: &'a oxc_allocator::Allocator,
    window: &ChunkWindow<'_>,
    chunk: &'a ChunkText<'_>,
    function_start: u32,
    free_names: Option<&mut FreeNames>,
) -> oxc_parser::ParserReturn<'a> {
    let Some(free_names) = free_names else {
        return lazy_bodies::parse(allocator, &chunk.text, SourceType::script(), None, None, None, false);
    };
    let strict = window.outer.is_some_and(|(_, scopes)| scopes.strict());
    lazy_bodies::parse_chunk(allocator, &chunk.text, function_start - chunk.start, chunk.start, strict, free_names)
}

/// What the analysis of the chunk's text, a function alone, finds: its private names, which
/// a chunk appended to a program makes none of, and the function expressions whose `prototype`
/// nothing observes. `None` where the text does not check alone. A text the load's skim decided
/// (`skimmed`, with the `function` keywords its own function spells) has no early error left
/// to find, and no private name or function expression to analyze unless it names one.
fn chunk_analysis(parsed: &oxc_parser::ParserReturn<'_>, name: &str, skimmed: Option<(&str, usize)>) -> Option<Starts> {
    if !chunk_parses_alone(parsed, skimmed.is_some()) {
        return None;
    }
    if skimmed.is_some_and(|(text, own)| !text.contains('#') && text.matches("function").count() <= own) {
        return Some(Starts::default());
    }
    let semantic = semantic_stats::checked_semantic(&parsed.program, name).ok()?;
    let private_name_ids = private_name_ids(&semantic.semantic);
    let unobservable = call_only::prototype_unobservable_functions(&semantic.semantic, &parsed.program);
    private_name_ids.is_empty().then_some(unobservable)
}

/// Whether the chunk's text, a function alone, parsed and checked as a script; the early errors
/// of the program's analysis are the load's where the load `skimmed` it.
fn chunk_parses_alone(parsed: &oxc_parser::ParserReturn<'_>, skimmed: bool) -> bool {
    !parsed.stack_exhausted
        && parsed.diagnostics.is_empty()
        && stack::validate(&parsed.program).is_ok()
        && early::regexp_early_error(parsed).is_none()
        && annex_b_targets::invalid_parsed_target(parsed).is_none()
        && (skimmed || early::program_early_errors(&parsed.program, false).1.is_empty())
}

impl<'a> Compiler<'a> {
    /// Makes the tables this compiler fills follow `program`'s, whose entries it reads.
    fn follow(&mut self, program: &'a ResidualProgram, replay: &'a replay::ProgramReplay) {
        let base = program.table_lengths();
        let first = base.functions as u32;
        self.functions = FunctionTable::chunk(&program.functions);
        self.function_exits = FunctionRecords::from_base(first);
        self.function_facts = FunctionRecords::from_base(first);
        self.constants = SiteTable::delta(&program.constants, base.constants);
        self.constant_index = ConstantIndex::delta(replay.views.constants.get_or_init(|| constant_hashes(&program.constants)), &program.constants);
        // The program keeps its method sites flattened, which the chunk reads none of.
        self.method_sites = SiteTable::delta(&[], base.method_sites);
        self.field_sites = SiteTable::delta(&program.field_sites, base.field_sites);
        self.object_sites = SiteTable::delta(&program.object_sites, base.object_sites);
        self.regexp_literal_sites = SiteTable::delta(&program.regexp_literal_sites, base.regexp_literal_sites);
        self.literal_templates = SiteTable::delta(&program.literal_templates, base.literal_templates);
        self.superinstructions = SiteTable::delta(&program.superinstructions, base.superinstructions);
        self.cache_sites = base.sites;
        self.name_shared_sites = NameSharedSites::Chunk(&replay.name_shared_sites, &replay.chunk_name_sites);
    }

    /// `node`, the chunk's function, compiled as its stub's: a function of the stub's parent,
    /// which resolves every name the stub's scopes do not bind in the global environment.
    fn compile_chunk_function(&mut self, node: &oxc_ast::ast::Function<'_>, parent: u32) -> u32 {
        let expression = node.is_expression();
        let context = replay::PlainFunctionContext {
            expression,
            default_export: false,
            scopes: capture_scope::ScopeChain::new(Vec::new()),
            parent,
            with_depth: 0,
            inherited_strict: false,
            prototype_unobservable: expression && self.prototype_unobservable.contains(&node.span.start),
        };
        self.compile_plain_function(node, context)
    }

    /// The chunk compiled, as the tables it adds after `base`: its functions finish unlowered,
    /// their rewrites left to their first reads as an eval program's are
    /// (`defers_kept_rewrites`).
    fn finish_chunk(mut self, base: TableLengths) -> Option<ChunkDelta> {
        let first = base.functions as u32;
        let private_members = self.deferred_name_bindings.values().any(|deferred| deferred.names_private_members);
        if !self.errors.is_empty() || self.template_sites != 0 || private_members {
            return None;
        }
        let functions = self.lay_out_chunk(first)?;
        let (mut method_sites, method_arguments) = Self::flatten_method_sites(self.method_sites.into_vec());
        for site in &mut method_sites {
            site.argument_start += base.method_arguments as u32;
        }
        Some(ChunkDelta {
            base,
            first_atom: self.atoms.prefix_len(),
            atoms: self.atoms.table(),
            functions: functions.into_iter().map(FunctionSlot::unlowered).collect(),
            constants: self.constants.into_vec(),
            method_sites,
            method_arguments,
            field_sites: self.field_sites.into_vec(),
            object_sites: self.object_sites.into_vec(),
            regexp_literal_sites: self.regexp_literal_sites.into_vec(),
            literal_templates: self.literal_templates.into_vec(),
            stubs: std::mem::take(&mut self.first_call.nested),
            sites: self.cache_sites,
        })
    }

    /// The chunk's records with their activations laid out over the functions of the chunk
    /// alone, or `None` where a name of the chunk resolves dynamically.
    fn lay_out_chunk(&mut self, first: u32) -> Option<Vec<BcFunction>> {
        let mut functions = self.chunk_records()?;
        let mut facts = std::mem::take(&mut self.function_facts).into_vec();
        for fact in &mut facts {
            fact.parent = fact.parent.and_then(|parent| parent.checked_sub(first));
        }
        let dynamic = capture_layout::dynamic_name_resolution(&facts);
        if dynamic.iter().chain(&capture_layout::dynamic_name_scopes(&facts, &dynamic)).any(|dynamic| *dynamic) {
            return None;
        }
        let layouts = capture_layout::inputs(&facts, &dynamic, |at| capture_layout::mapped_arguments(&functions[at]));
        for (at, function) in functions.iter_mut().enumerate() {
            capture_layout::apply_function(function, (first as usize) + at, &layouts[at], &self.atoms);
            self.settle_dispatch(function);
            function.shrink_to_fit();
        }
        Some(functions)
    }

    /// The records of the functions the chunk made, none released.
    fn chunk_records(&mut self) -> Option<Vec<BcFunction>> {
        std::mem::take(&mut self.functions)
            .into_entries()
            .into_iter()
            .map(|entry| match entry {
                replay::FunctionEntry::Compiled(function) => Some(*function),
                _ => None,
            })
            .collect()
    }
}
