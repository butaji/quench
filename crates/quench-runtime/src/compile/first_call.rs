//! First-call stubs: a top-level function declaration, or an anonymous function expression
//! of a top-level statement, whose body was parsed lazily and that nothing at load calls
//! compiles as a stub, which compiles the function's source as a
//! program of its own on the function's first call (`CallFirstChunk`). The load then
//! checks the body's syntax and nothing more, as V8's preparser does.
//!
//! The chunk compiles the function alone as a Script, so it resolves every free name in the
//! global environment: only a root whose own bindings all live there stubs its functions, a
//! sloppy global eval's that declares no lexical binding. It parses the function's
//! text alone and compiles it at its place in the program's text, whose source it shares, so
//! a chunk costs its function's text, not the program's.
use super::*;
use crate::bytecode::{RETURN_OP, Traits, the_op};
use program_shape::ParsedProgramShape;
use oxc_ast_visit::Visit;
use replay::ShiftSpans;
mod append;
mod room;
mod template;
pub(in crate::compile) use template::StubOwn;
pub(crate) mod nested;
pub(crate) use nested::{FirstCallScopes, FirstCallStubs};
pub(in crate::compile) use nested::{FreeNameCheck, FreeNames, alone_body_end, alone_start, free_names, reaches_scopes_by_name, skimmed_free_names};
pub(in crate::compile) use room::ChunkRoom;

/// The op that runs a first-call stub on its chunk.
const FIRST_CALL_CHUNK: Op = the_op(Traits::FIRST_CALL_CHUNK);

/// A body the compiler makes for a function whose source gives it none to run.
#[derive(Clone, Copy)]
pub(super) enum ImplicitBody {
    /// A derived class's default constructor: `super(...args)`.
    Super,
    /// A first-call stub's.
    FirstCallStub,
    /// The first-call stub's of a function with no `new.target` of its own: an arrow
    /// function's, or a class method's.
    FirstCallArrowStub,
}

/// Where a first-call chunk's function lies (`ParsedProgramShape::FirstCallChunk`): at `start`
/// in the text of `source`.
#[derive(Clone, Copy)]
pub(super) struct ChunkWindow<'a> {
    source: &'a crate::bytecode::ProgramSource,
    start: u32,
    /// Where the stub's function starts in the program's text.
    function: u32,
    /// A nested stub's function start and the scopes it compiles against.
    outer: Option<(u32, &'a std::rc::Rc<FirstCallScopes>)>,
    /// The run of a streamed body the chunk compiles.
    run: Option<&'a super::statement_stream::StreamRun>,
    /// The load checked the function's body alone (`ProgramReplay::checked_at_load`), so its early
    /// errors are settled and the chunk's analysis skips their check.
    checked: bool,
    /// The program's declared environment, which the chunk's function's declarations are read from
    /// (`declared`), and the end of the function's source range in the program's text.
    program_declared: Option<&'a crate::bytecode::DeclaredTypes>,
    end: u32,
}

impl<'a> ChunkWindow<'a> {
    /// Whether the chunk compiles the first run of a streamed body.
    pub(super) fn first_run(&self) -> bool {
        self.run.is_some_and(|run| run.from.is_none())
    }

    /// Whether the load checked the function's body (`checked`).
    pub(super) fn checked(&self) -> bool {
        self.checked
    }

    pub(super) fn start(&self) -> u32 {
        self.start
    }

    /// The program's saved top-level type names, which the chunk's annotations resolve against.
    pub(super) fn saved_names(&self) -> Option<std::rc::Rc<[crate::bytecode::SavedTypeName]>> {
        self.program_declared.and_then(|declared| declared.type_names.clone())
    }

    /// The declarations of the chunk's function and of what lies in it, at their places in the
    /// program's text (`DeclaredTypes::within`): a chunk compiles them as the program's load did.
    pub(super) fn declared(&self) -> crate::bytecode::DeclaredTypes {
        self.program_declared.map_or_else(Default::default, |declared| declared.within(self.function, self.end))
    }

    /// Whether the chunk compiles a run of a streamed body.
    pub(super) fn streamed(&self) -> bool {
        self.run.is_some()
    }

    /// Where in the chunk's text lazily parsed bodies begin: past the function's own start,
    /// whose body the chunk compiles now, or from the start in a streamed body's chunk.
    pub(super) fn lazy_from(&self) -> u32 {
        if self.streamed() { 0 } else { self.function - self.start + 1 }
    }

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
        let mut compiler = Compiler::new(name, &self.source.text, mode, atom_prefix, private_name_ids);
        compiler.first_call.kept_source = Some(self.source);
        compiler.first_call.outer = self.outer.map(|(start, scopes)| (start, scopes.clone()));
        compiler.first_call.chunk_function = Some(self.function);
        compiler.stream_run = self.run;
        compiler
    }
}

/// The length of the `({` a class method's chunk text is wrapped in, before its place.
const METHOD_WRAPPER_PREFIX: u32 = 2;

/// A stub's function text as its chunk compiles it, with where the function starts in the
/// program's text. An arrow function, or an anonymous function expression, is no statement: it compiles
/// parenthesized, one character before its place, so the function itself starts where it did.
pub(super) struct ChunkText<'a> {
    pub(super) text: std::borrow::Cow<'a, str>,
    pub(super) start: u32,
}

impl<'a> ChunkText<'a> {
    /// The chunk text of the stub at `span`; a class method's (`method`) compiles as an object
    /// literal's one method, two characters before its place.
    pub(super) fn of(source: &'a crate::bytecode::ProgramSource, span: crate::bytecode::SourceSpan, method: bool) -> Option<Self> {
        let snippet = source.text.get(span.start as usize..span.end as usize)?;
        if method {
            let start = span.start.checked_sub(METHOD_WRAPPER_PREFIX)?;
            return Some(Self { text: std::borrow::Cow::Owned(format!("({{{snippet}}})")), start });
        }
        let anonymous = !snippet.starts_with("function")
            || !snippet["function".len()..].trim_start().starts_with(|c: char| c == '_' || c == '$' || c.is_alphanumeric() || !c.is_ascii());
        Some(if anonymous && span.start > 0 {
            Self { text: std::borrow::Cow::Owned(format!("({snippet})")), start: span.start - 1 }
        } else {
            Self { text: std::borrow::Cow::Borrowed(snippet), start: span.start }
        })
    }
}

impl Engine {
    /// The chunk of stub `function` of `program`, at `span`, as a program of its own
    /// (`Vm::call_first_chunk`).
    pub(crate) fn specialize_first_call_chunk(
        program: &ResidualProgram,
        function: u32,
        span: crate::bytecode::SourceSpan,
        atom_prefix: &dyn AtomPrefix,
        run: Option<&super::statement_stream::StreamRun>,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let name = program.source_name.as_str();
        let source = program.source.as_ref().filter(|source| source.base == 0);
        fn chunk_text<'s>(source: &'s crate::bytecode::ProgramSource, span: crate::bytecode::SourceSpan, run: Option<&super::statement_stream::StreamRun>, method: bool) -> Option<ChunkText<'s>> { match run.and_then(|run| run.from) {
            // A later run's chunk holds the function's header alone, its body from the run on.
            Some((_, body)) => source
                .text
                .get(span.start as usize..body.open as usize)
                .map(|header| ChunkText { text: std::borrow::Cow::Owned(format!("{header}{{}}")), start: span.start }),
            None => ChunkText::of(source, span, method),
        } }
        let method = program.functions.get(function as usize).is_some_and(|stub| stub_is_method(&stub.header()));
        let Some((source, chunk)) = source.and_then(|source| Some((source, chunk_text(source, span, run, method)?))) else {
            return Err(vec![Diagnostic::unsupported(name, "a first-call stub keeps no source")]);
        };
        let stubs = &program.first_call_scopes;
        let outer = stubs.binary_search_by_key(&function, |(stub, _)| *stub).ok().map(|at| (span.start, &stubs[at].1));
        let checked = program.replay.as_deref().is_some_and(|replay| replay.checked_at_load(span.start));
        let program_declared = program.declared_types.as_deref();
        let window = ChunkWindow { source, start: chunk.start, function: span.start, outer, run, checked, program_declared, end: span.end };
        Self::specialize_script_shaped(&chunk.text, name, atom_prefix, ParsedProgramShape::FirstCallChunk(window), ReleasedRecords::Dropped, SourceType::script().with_typescript(source.typescript), None)
    }
}

/// Whether a stub's function is a class method's: the only stub neither an arrow nor
/// constructible (`compile_first_call_method_stub`).
pub(super) fn stub_is_method(header: &crate::bytecode::FunctionHeader) -> bool {
    !header.is_arrow && !header.constructible
}

/// The rest parameter that holds a stub's arguments.
pub(in crate::compile) const STUB_ARGUMENTS: &str = "\0quench:first-call-args";

/// Functions by their starts in the program's text: sparse offsets, so a set hashes them.
type Starts = FxHashSet<u32>;

impl ParsedProgramShape<'_> {
    /// The starts of the function declarations of `program`, of this shape, that may compile
    /// as first-call stubs: an eval's whose functions compile again from their text alone
    /// (`replayable`), or a large classic script's (`stubbing`), when its root is sloppy and
    /// declares no lexical binding. A Function-constructor body's declarations are not roots:
    /// their free names are the body's own bindings, which a root's chunk, compiled as a script,
    /// would resolve in the global environment. They stub as nested stubs (`nested_stub_captures`).
    pub(super) fn first_call_roots(self, replayable: bool, program: &Program<'_>, inherited_strict: bool) -> Starts {
        let strict = inherited_strict || program.directives.iter().any(|directive| directive.directive == "use strict");
        let lexical = program.body.iter().any(|statement| match statement {
            Statement::VariableDeclaration(declaration) => !declaration.kind.is_var(),
            Statement::ClassDeclaration(_) => true,
            _ => false,
        });
        let script = matches!(self, Self::FirstCallScript) && self.stubbing(replayable, program.source_text.len(), program.source_type.is_module()).functions;
        if !replayable || !(matches!(self, Self::Eval(..)) || script) || strict || lexical {
            return FxHashSet::default();
        }
        TopLevelFunctions::of(program)
    }
}

impl ParsedProgramShape<'_> {
    /// The starts of the named function declarations of `program`, of this shape, that may
    /// compile as stubs where their bodies stream (`Stubs::streamed_roots`): a sloppy
    /// script's, whose lexical bindings are global too, or an eval's that declares none.
    fn streamed_roots(self, replayable: bool, program: &Program<'_>, inherited_strict: bool) -> Starts {
        let strict = inherited_strict || program.directives.iter().any(|directive| directive.directive == "use strict");
        let lexical = program.body.iter().any(|statement| match statement {
            Statement::VariableDeclaration(declaration) => !declaration.kind.is_var(),
            Statement::ClassDeclaration(_) => true,
            _ => false,
        });
        let shaped = match self {
            Self::Any | Self::FirstCallScript => !program.source_type.is_module(),
            Self::Eval(..) => !lexical,
            _ => false,
        };
        if !replayable || strict || !shaped {
            return FxHashSet::default();
        }
        program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) if function.id.is_some() => Some(function.span.start),
                _ => None,
            })
            .collect()
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
    /// Nested functions may compile as stubs: the source is an indirect eval's. Its lazily
    /// parsed bodies' call-only facts are deferred (`call_only::deferred`).
    pub(super) nested_allowed: bool,
    /// The stubs include arrow functions, and the parser leaves the bodies it does not skim
    /// whole (`FreeNameCheck`): a direct eval's source, and its chunks.
    pub(super) direct: bool,
    /// The functions whose deferred call-only facts are found, by source range.
    pub(super) call_only_found: Vec<(u32, u32)>,
    /// The starts of the root's named function declarations that compile as stubs where
    /// their bodies stream, their first call then running the body a run at a time
    /// (`Vm::call_streamed_runs`).
    pub(super) streamed_roots: Starts,
    /// The free names of the lazily parsed functions (`BodyFacts::check`).
    pub(super) free_names: FreeNames,
    /// The nested stubs by id, in order, with the scopes their chunks compile against.
    pub(super) nested: Vec<(u32, std::rc::Rc<FirstCallScopes>)>,
    /// In a chunk, where its own function starts: compiled now, never a stub again.
    pub(super) chunk_function: Option<u32>,
    /// The stub records later stubs copy (`template`).
    pub(super) templates: template::StubTemplates,
    /// In a chunk, its stub's function start and the scopes it compiles against.
    pub(super) outer: Option<(u32, std::rc::Rc<FirstCallScopes>)>,
    /// The room the stubs' chunks may append to the program in.
    pub(super) room: ChunkRoom,
}

impl Stubs<'_> {
    /// Plans the stubs of a program of `shape`: its root functions, and its nested ones when
    /// the source is an indirect eval's, whose captures `free_names` predict.
    pub(super) fn plan(&mut self, shape: ParsedProgramShape<'_>, replayable: bool, program: &Program<'_>, inherited_strict: bool, (free_names, call_only_found): (FreeNames, Vec<(u32, u32)>)) {
        self.roots = shape.first_call_roots(replayable, program, inherited_strict);
        let stubbing = shape.stubbing(replayable, program.source_text.len(), program.source_type.is_module());
        self.streamed_roots = shape.streamed_roots(replayable, program, inherited_strict);
        self.nested_allowed = stubbing.functions;
        self.direct = stubbing.arrows;
        self.free_names = free_names;
        self.call_only_found = call_only_found;
        super::call_only::deferred::normalize_spans(&mut self.call_only_found);
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
            self.note_nested_stub(id, captures);
            return id;
        }
        if self.streams_at_first_call(function, &context) {
            return self.compile_first_call_stub(function, context);
        }
        self.lazy_compiled.push(body);
        self.find_deferred_call_only_facts(function, &context);
        self.compile_lazy_function(function, context)
    }

    /// Whether `function`, whose body was parsed lazily, compiles as a first-call stub whose
    /// call runs its body a run at a time: a root's whose body streams.
    fn streams_at_first_call(&self, function: &oxc_ast::ast::Function<'_>, context: &replay::PlainFunctionContext) -> bool {
        self.first_call.streamed_roots.contains(&function.span.start)
            && self.replay_target.is_none()
            && self.stream_run.is_none()
            && (self.streamed.contains(&function.span.start)
                || self.stream_candidates.contains(&function.span.start) && self.body_streams(function, context))
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
        self.first_call.room.add_stub(function.span.end - function.span.start);
        let strict = context.inherited_strict || function
            .body
            .as_ref()
            .is_some_and(|body| body.directives.iter().any(|directive| directive.directive == "use strict"));
        let name = function.id.as_ref().map(|id| id.name.as_str());
        let source_span = self.source_span(function.span);
        let own = StubOwn { body: ImplicitBody::FirstCallStub, strict, name, parent: Some(context.parent), source_span, constructible: true };
        let id = self.templated_stub(own, |compiler| compiler.compile_function(
            name,
            &[STUB_ARGUMENTS.to_owned()],
            FunctionBody::Statements(&[]),
            &context.scopes,
            Some(context.parent),
            FunctionOptions {
                source_span,
                rest_override: true,
                implicit_body: Some(ImplicitBody::FirstCallStub),
                strict,
                ..FunctionOptions::default()
            },
        ));
        if let Some(stub) = self.functions.get_mut(id) {
            stub.length = Self::formal_length(&function.params) as u16;
        }
        id
    }
}

impl Compiler<'_> {
    /// `arrow`, whose body was parsed lazily, compiled as its first-call stub: an arrow function
    /// of its length and strictness, whose first call forwards to its chunk.
    pub(super) fn compile_first_call_arrow_stub(&mut self, arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>, context: &replay::ArrowContext) -> u32 {
        self.first_call.room.add_stub(arrow.span.end - arrow.span.start);
        let strict = context.inherited_strict
            || matches!(&arrow.body, oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) if body.directives.iter().any(|directive| directive.directive == "use strict"));
        let source_span = self.source_span(arrow.span);
        let own = StubOwn { body: ImplicitBody::FirstCallArrowStub, strict, name: None, parent: context.parent, source_span, constructible: true };
        let id = self.templated_stub(own, |compiler| compiler.compile_function(
            None,
            &[STUB_ARGUMENTS.to_owned()],
            FunctionBody::Statements(&[]),
            &context.scopes,
            context.parent,
            FunctionOptions {
                source_span,
                rest_override: true,
                implicit_body: Some(ImplicitBody::FirstCallArrowStub),
                strict,
                ..FunctionOptions::default()
            },
        ));
        if let Some(stub) = self.functions.get_mut(id) {
            stub.length = Self::formal_length(&arrow.params) as u16;
            stub.is_arrow = true;
            stub.constructible = false;
        }
        id
    }

    /// The arrow function of a chunk's text, compiled as its stub's: of the stub's parent,
    /// resolving the names the stub's scopes do not bind in the global environment.
    pub(super) fn compile_chunk_arrow(&mut self, node: &oxc_ast::ast::ArrowFunctionExpression<'_>, parent: u32) -> u32 {
        let context = replay::ArrowContext {
            scopes: capture_scope::ScopeChain::new(Vec::new()),
            parent: Some(parent),
            super_static: false,
            super_home: false,
            super_home_atom: None,
            inherited_strict: false,
            class_initializer: false,
            lexical_this_atom: None,
            super_call_binds_this: false,
            with_depth: 0,
        };
        self.compile_replayable_arrow(node, context)
    }
}

impl FunctionCompiler<'_, '_> {
    /// `body`, which returns: a stub's calls its chunk with its arguments.
    pub(super) fn emit_implicit_body(&mut self, body: ImplicitBody) {
        match body {
            ImplicitBody::Super => self.emit_implicit_super(),
            ImplicitBody::FirstCallArrowStub => {
                let arguments = self.load_name(STUB_ARGUMENTS);
                let result = self.reg();
                self.emit(FIRST_CALL_CHUNK, result, arguments, arguments, 0);
                self.emit(RETURN_OP, result, 0, 0, 0);
            }
            ImplicitBody::FirstCallStub => {
                let arguments = self.load_name(STUB_ARGUMENTS);
                let new_target = self.load_name(crate::bytecode::NEW_TARGET_BINDING);
                let result = self.reg();
                self.emit(FIRST_CALL_CHUNK, result, arguments, new_target, 0);
                self.emit(RETURN_OP, result, 0, 0, 0);
            }
        }
    }
}
