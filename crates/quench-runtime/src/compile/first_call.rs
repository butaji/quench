//! First-call stubs: a top-level function declaration, or an anonymous function expression
//! of a top-level statement, whose body was parsed lazily and that nothing at load calls
//! compiles as a stub, which compiles the function's source as a
//! program of its own on the function's first call (`Op::CallFirstChunk`). The load then
//! checks the body's syntax and nothing more, as V8's preparser does.
//!
//! The chunk compiles the function alone as a Script, so it resolves every free name in the
//! global environment: only a root whose own bindings all live there stubs its functions, a
//! sloppy global eval's that declares no lexical binding. It parses the function's
//! text alone and compiles it at its place in the program's text, whose source it shares, so
//! a chunk costs its function's text, not the program's.
use super::*;
use program_shape::ParsedProgramShape;
use oxc_ast_visit::Visit;
use replay::{ReplayViews, ShiftSpans};
mod nested;
pub(crate) use nested::{FirstCallScopes, FirstCalls, ScopeLevel};
pub(in crate::compile) use nested::{FreeNameCheck, FreeNames, free_names};

impl ReplayViews {
    /// The line starts and UTF-16 excess table of `text`, the program's source, which a
    /// replay and a first-call chunk compile with.
    pub(super) fn layout(&self, text: &str) -> (&[usize], &[(usize, usize)]) {
        (
            self.line_starts.get_or_init(|| crate::bytecode::source_line_starts(text).collect()),
            self.utf16_excess.get_or_init(|| super::source_utf16_excess(text)),
        )
    }
}

/// A body the compiler makes for a function whose source gives it none to run.
#[derive(Clone, Copy)]
pub(super) enum ImplicitBody {
    /// A derived class's default constructor: `super(...args)`.
    Super,
    /// A first-call stub's.
    FirstCallStub,
}

/// Where a first-call chunk's function lies (`ParsedProgramShape::FirstCallChunk`): at `start`
/// in the text of `source`, whose layout tables a replay of its program shares.
#[derive(Clone, Copy)]
pub(super) struct ChunkWindow<'a> {
    source: &'a crate::bytecode::ProgramSource,
    start: u32,
    /// A nested stub's function start and the scopes it compiles against.
    outer: Option<(u32, &'a std::rc::Rc<FirstCallScopes>)>,
    line_starts: &'a [usize],
    utf16_excess: &'a [(usize, usize)],
}

impl<'a> ChunkWindow<'a> {
    /// Moves the positions of `program`, the function's text parsed alone, and of the facts
    /// its analysis found, to the function's place in the program's text.
    fn place(
        self,
        program: &mut Program<'_>,
        private_name_ids: &mut FxHashMap<(u32, u32), u32>,
        private_name_labels: &mut FxHashMap<(u32, u32), String>,
        prototype_unobservable: &mut FxHashSet<u32>,
    ) {
        let delta = self.start;
        oxc_ast_visit::VisitMut::visit_program(&mut ShiftSpans(delta), program);
        for comment in program.comments.iter_mut() {
            ShiftSpans(delta).shift(&mut comment.span);
        }
        let shift = |(start, end): (u32, u32)| (start + delta, end + delta);
        *private_name_ids = std::mem::take(private_name_ids).into_iter().map(|(span, id)| (shift(span), id)).collect();
        *private_name_labels = std::mem::take(private_name_labels).into_iter().map(|(span, label)| (shift(span), label)).collect();
        *prototype_unobservable = std::mem::take(prototype_unobservable).into_iter().map(|start| start + delta).collect();
    }

    /// The compiler of the chunk, over the program's text, with `program` (the function's
    /// text parsed alone) and the facts its analysis found placed in it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compiler(
        self,
        name: &'a str,
        mode: SpecializationMode,
        atom_prefix: &'a dyn AtomPrefix,
        program: &mut Program<'_>,
        mut private_name_ids: FxHashMap<(u32, u32), u32>,
        private_name_labels: &mut FxHashMap<(u32, u32), String>,
        prototype_unobservable: &mut FxHashSet<u32>,
    ) -> Compiler<'a> {
        self.place(program, &mut private_name_ids, private_name_labels, prototype_unobservable);
        let mut compiler = Compiler::with_layout(
            name,
            &self.source.text,
            mode,
            atom_prefix,
            private_name_ids,
            std::borrow::Cow::Borrowed(self.line_starts),
            std::borrow::Cow::Borrowed(self.utf16_excess),
        );
        compiler.first_call.kept_source = Some(self.source);
        compiler.first_call.outer = self.outer.map(|(start, scopes)| (start, scopes.clone()));
        compiler
    }
}

impl Engine {
    /// The chunk of stub `function` of `program`, at `span` (`Vm::call_first_chunk`).
    pub(crate) fn specialize_first_call_chunk(
        program: &ResidualProgram,
        function: u32,
        span: crate::bytecode::SourceSpan,
        atom_prefix: &dyn AtomPrefix,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let name = program.source_name.as_str();
        let source = program.source.as_ref().filter(|source| source.base == 0);
        let Some((source, snippet)) = source.and_then(|source| Some((source, source.text.get(span.start as usize..span.end as usize)?))) else {
            return Err(vec![Diagnostic::unsupported(name, "a first-call stub keeps no source")]);
        };
        let views = ReplayViews::default();
        let (line_starts, utf16_excess) = program.replay.as_deref().map_or(&views, |replay| &replay.views).layout(&source.text);
        // An anonymous function expression is no statement: it compiles parenthesized, one
        // character before its place, so the function itself starts where it did.
        let anonymous = !snippet["function".len().min(snippet.len())..].trim_start().starts_with(|c: char| c == '_' || c == '$' || c.is_alphanumeric() || !c.is_ascii());
        let (snippet, start) = if anonymous && span.start > 0 { (std::borrow::Cow::Owned(format!("({snippet})")), span.start - 1) } else { (std::borrow::Cow::Borrowed(snippet), span.start) };
        let calls = &program.first_calls;
        let outer = calls.binary_search_by_key(&function, |(stub, _)| *stub).ok().map(|at| (span.start, &calls[at].1));
        let window = ChunkWindow { source, start, outer, line_starts, utf16_excess };
        Self::specialize_script_shaped(&snippet, name, atom_prefix, ParsedProgramShape::FirstCallChunk(window), ReleasedRecords::Dropped, SourceType::script())
    }
}

/// The rest parameter that holds a stub's arguments.
const STUB_ARGUMENTS: &str = "\0quench:first-call-args";

/// Functions by their starts in the program's text: sparse offsets, so a set hashes them.
type Starts = FxHashSet<u32>;

impl ParsedProgramShape<'_> {
    /// The starts of the function declarations of `program`, of this shape, that may compile
    /// as first-call stubs: an eval's whose functions compile again from their text alone
    /// (`replayable`), when its root is sloppy and declares no lexical binding.
    pub(super) fn first_call_roots(self, replayable: bool, program: &Program<'_>, inherited_strict: bool) -> Starts {
        let strict = inherited_strict || program.directives.iter().any(|directive| directive.directive == "use strict");
        let lexical = program.body.iter().any(|statement| match statement {
            Statement::VariableDeclaration(declaration) => !declaration.kind.is_var(),
            Statement::ClassDeclaration(_) => true,
            _ => false,
        });
        if !replayable || !matches!(self, Self::Eval(..)) || strict || lexical {
            return FxHashSet::default();
        }
        TopLevelFunctions::of(program)
    }
}

/// The anonymous function expressions of a top-level statement's expressions, outside every
/// function and class: their free names are the root's, all global. Anonymous, a chunk tells
/// one from a declaration by its text (`specialize_first_call_chunk`).
struct TopLevelFunctions(Starts);

impl TopLevelFunctions {
    /// The starts of `program`'s named function declarations and of the anonymous function
    /// expressions of its expression statements and variable initializers.
    fn of(program: &Program<'_>) -> Starts {
        let mut roots = Self(FxHashSet::default());
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) if function.id.is_some() => {
                    roots.0.insert(function.span.start);
                }
                Statement::ExpressionStatement(statement) => roots.visit_expression(&statement.expression),
                Statement::VariableDeclaration(declaration) => roots.visit_variable_declaration(declaration),
                _ => {}
            }
        }
        roots.0
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for TopLevelFunctions {
    fn visit_function(&mut self, function: &oxc_ast::ast::Function<'a>, _: oxc_syntax::scope::ScopeFlags) {
        if function.is_expression() && function.id.is_none() {
            self.0.insert(function.span.start);
        }
    }

    fn visit_arrow_function_expression(&mut self, _: &oxc_ast::ast::ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _: &oxc_ast::ast::Class<'a>) {}
}

/// What a compilation knows of first-call stubs (`Compiler::first_call`).
#[derive(Default)]
pub(super) struct Stubs<'a> {
    /// The starts of the root's function declarations that may compile as stubs.
    pub(super) roots: Starts,
    /// The source of the program a chunk compiles from, which the chunk shares.
    pub(super) kept_source: Option<&'a crate::bytecode::ProgramSource>,
    /// Nested functions may compile as stubs: the source is an indirect eval's.
    pub(super) nested_allowed: bool,
    /// The free names of the lazily parsed functions (`BodyFacts::check`).
    pub(super) free_names: FreeNames,
    /// The nested stubs by id, in order, with the scopes their chunks compile against.
    pub(super) nested: Vec<(u32, std::rc::Rc<FirstCallScopes>)>,
    /// The scope levels named so far, by scope (`Compiler::scope_level`).
    levels: FxHashMap<usize, (std::rc::Rc<capture_scope::CaptureScope>, nested::ScopeLevel)>,
    /// In a chunk, its stub's function start and the scopes it compiles against.
    pub(super) outer: Option<(u32, std::rc::Rc<FirstCallScopes>)>,
}

impl Stubs<'_> {
    /// Plans the stubs of a program of `shape`: its root functions, and its nested ones when
    /// the source is an indirect eval's, whose captures `free_names` predict.
    pub(super) fn plan(&mut self, shape: ParsedProgramShape<'_>, replayable: bool, program: &Program<'_>, inherited_strict: bool, free_names: FreeNames) {
        self.roots = shape.first_call_roots(replayable, program, inherited_strict);
        self.nested_allowed = replayable && matches!(shape, ParsedProgramShape::Eval(..));
        self.free_names = free_names;
    }

    /// Whether a program keeps a replay: it has functions a replay compiles (unless
    /// `no_replays`), or first-call stubs, whose chunks compile with its layout tables.
    pub(super) fn keep_replay(&self, no_replays: bool) -> bool {
        !no_replays || !self.roots.is_empty() || !self.nested.is_empty()
    }
}

impl Compiler<'_> {
    /// `function`, whose body at `body` was parsed lazily: its first-call stub, or the
    /// function compiled from its body parsed again (`compile_lazy_function`).
    pub(super) fn compile_released_function(&mut self, function: &oxc_ast::ast::Function<'_>, body: u32, context: replay::PlainFunctionContext) -> u32 {
        if self.stubs_first_call(function) {
            return self.compile_first_call_stub(function, context);
        }
        if let Some(captures) = self.nested_stub_captures(function, &context) {
            let id = self.compile_first_call_stub(function, context.clone());
            self.note_nested_stub(id, captures, &context);
            return id;
        }
        self.lazy_compiled.push(body);
        self.compile_lazy_function(function, context)
    }

    /// Whether `function`, whose body was parsed lazily, compiles as a first-call stub.
    fn stubs_first_call(&self, function: &oxc_ast::ast::Function<'_>) -> bool {
        self.first_call.roots.contains(&function.span.start)
            && self.replay_target.is_none()
            && !function.r#async
            && !function.generator
            && !self.invoked_functions.contains(&function.span.start)
    }

    /// `function` compiled as its first-call stub: a function of its name, length,
    /// strictness and source that forwards its first call to its chunk.
    fn compile_first_call_stub(&mut self, function: &oxc_ast::ast::Function<'_>, context: replay::PlainFunctionContext) -> u32 {
        let strict = context.inherited_strict || function
            .body
            .as_ref()
            .is_some_and(|body| body.directives.iter().any(|directive| directive.directive == "use strict"));
        let id = self.compile_function(
            function.id.as_ref().map(|id| id.name.as_str()),
            &[STUB_ARGUMENTS.to_owned()],
            FunctionBody::Statements(&[]),
            &context.scopes,
            Some(context.parent),
            FunctionOptions {
                source_span: self.source_span(function.span),
                rest_override: true,
                implicit_body: Some(ImplicitBody::FirstCallStub),
                strict,
                ..FunctionOptions::default()
            },
        );
        if let Some(stub) = self.functions.get_mut(id) {
            stub.length = Self::formal_length(&function.params) as u16;
        }
        id
    }
}

impl FunctionCompiler<'_, '_> {
    /// `body`, which returns: a stub's calls its chunk with its arguments.
    pub(super) fn emit_implicit_body(&mut self, body: ImplicitBody) {
        match body {
            ImplicitBody::Super => self.emit_implicit_super(),
            ImplicitBody::FirstCallStub => {
                let arguments = self.load_name(STUB_ARGUMENTS);
                let result = self.reg();
                self.emit(Op::CallFirstChunk, result, arguments, 0, 0);
                self.emit(Op::Return, result, 0, 0, 0);
            }
        }
    }
}
