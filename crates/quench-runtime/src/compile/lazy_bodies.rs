//! Function bodies parsed lazily (TOD-929). The parser releases the arena of each
//! outermost plain function body and keeps it as its directives
//! (`ParseOptions::lazy_function_bodies`); the compiler parses each again, alone, at the
//! point it compiles that function, so the whole program's tree never exists at once.
//!
//! A kept body stands in for the one released with a direct `eval()` call, which every
//! analysis of the code around a function already reads as "may reference and assign any
//! visible binding": what that code concludes about the body is what it would conclude
//! for any body. The function itself compiles from its real body. A body that fails an
//! early check alone makes the compilation fail, and the program compiles again eagerly,
//! so its diagnostics are the eager ones.
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, ExpressionStatement, Function, FunctionBody,
    IdentifierReference, Program, Statement,
};
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::SourceType;
use rustc_hash::FxHashSet;
#[cfg(feature = "verify-replay")]
use {
    oxc_ast::ast::{Class, FunctionType, MethodDefinition, ObjectProperty, ParenthesizedExpression, PropertyKind},
    oxc_ast_visit::{Visit, walk},
    oxc_syntax::scope::ScopeFlags,
};

#[cfg(feature = "verify-replay")]
/// A body that may be validated alone.
#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    /// The function's source range.
    pub(super) start: u32,
    pub(super) end: u32,
    /// The body's text after its directive prologue, up to its closing brace.
    pub(super) statements_start: u32,
    pub(super) statements_end: u32,
    /// Parses again as an expression, not a declaration.
    pub(super) expression: bool,
    /// The strictness the function inherits from the code around it.
    pub(super) inherited_strict: bool,
}

#[cfg(feature = "verify-replay")]
/// The candidates of `program`, in source order.
pub(super) fn candidates(program: &Program<'_>) -> Vec<Candidate> {
    let mut finder = Candidates {
        strict: program.source_type.is_module() || has_use_strict(&program.directives),
        not_candidate: false,
        found: Vec::new(),
    };
    finder.visit_program(program);
    finder.found
}


#[cfg(feature = "verify-replay")]
struct Candidates {
    strict: bool,
    /// The next function visited is a method, an accessor or a parenthesized expression.
    not_candidate: bool,
    found: Vec<Candidate>,
}

#[cfg(feature = "verify-replay")]
impl<'a> Visit<'a> for Candidates {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let not_candidate = std::mem::take(&mut self.not_candidate);
        let Some(body) = function.body.as_deref() else {
            return walk::walk_function(self, function, flags);
        };
        let plain = matches!(
            function.r#type,
            FunctionType::FunctionDeclaration | FunctionType::FunctionExpression
        );
        if plain && !not_candidate {
            let statements_start = body
                .directives
                .last()
                .map_or(body.span.start + 1, |directive| directive.span.end);
            self.found.push(Candidate {
                start: function.span.start,
                end: function.span.end,
                statements_start,
                statements_end: body.span.end - 1,
                expression: function.r#type == FunctionType::FunctionExpression,
                inherited_strict: self.strict,
            });
            return;
        }
        let strict = self.strict;
        self.strict |= has_use_strict(&body.directives);
        walk::walk_function(self, function, flags);
        self.strict = strict;
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        let strict = std::mem::replace(&mut self.strict, true);
        walk::walk_class(self, class);
        self.strict = strict;
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'a>) {
        self.visit_property_key(&method.key);
        self.not_candidate = true;
        self.visit_function(&method.value, ScopeFlags::empty());
        for decorator in &method.decorators {
            self.visit_decorator(decorator);
        }
    }

    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        self.visit_property_key(&property.key);
        self.not_candidate = property.method || property.kind != PropertyKind::Init;
        self.visit_expression(&property.value);
        self.not_candidate = false;
    }

    fn visit_parenthesized_expression(&mut self, expression: &ParenthesizedExpression<'a>) {
        // A parenthesized function is about to be called, as engines guess.
        self.not_candidate = matches!(expression.expression, Expression::FunctionExpression(_));
        self.visit_expression(&expression.expression);
        self.not_candidate = false;
    }

    fn visit_export_default_declaration(
        &mut self,
        declaration: &oxc_ast::ast::ExportDefaultDeclaration<'a>,
    ) {
        // `export default function` parses again only in its own wrapper; not a candidate yet.
        self.not_candidate = true;
        walk::walk_export_default_declaration(self, declaration);
        self.not_candidate = false;
    }
}

#[cfg(feature = "verify-replay")]
/// Whether `source` parses and passes every early check a program's compilation applies
/// to the syntax of function bodies.
pub(super) fn passes_early_checks(source: &str, source_type: SourceType) -> bool {
    let allocator = Allocator::with_capacity(source.len().saturating_mul(6));
    let parsed = super::annex_b_targets::parse_program(&allocator, source, source_type);
    !parsed.stack_exhausted
        && parsed.diagnostics.is_empty()
        && super::stack::validate(&parsed.program).is_ok()
        && super::early::regexp_early_error(&parsed.program).is_none()
        && super::annex_b_targets::invalid_target(&parsed.program).is_none()
        && oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .is_empty()
}

#[cfg(feature = "verify-replay")]
/// Whether `candidate`'s text in `source`, parsed alone with the context it inherits,
/// passes the early checks.
pub(super) fn validates_alone(source: &str, source_type: SourceType, candidate: Candidate) -> bool {
    let snippet = &source[candidate.start as usize..candidate.end as usize];
    let strict = if candidate.inherited_strict && !source_type.is_module() {
        "\"use strict\";"
    } else {
        ""
    };
    let (prefix, suffix) = if candidate.expression { ("(", ")") } else { ("", "") };
    passes_early_checks(&format!("{strict}{prefix}{snippet}{suffix}"), source_type)
}

/// Checks, under `QUENCH_LAZY_BODIES_VERIFY`, that validating each candidate of `source`
/// alone loses no early error: the program passes the early checks exactly when it does
/// with the bodies that validated alone emptied (directives kept, offsets kept).
#[cfg(feature = "verify-replay")]
pub(super) fn verify(name: &str, source: &str, program: &Program<'_>) {
    if std::env::var_os("QUENCH_LAZY_BODIES_VERIFY").is_none() {
        return;
    }
    let source_type = program.source_type;
    let candidates = candidates(program);
    let mut emptied = source.as_bytes().to_vec();
    let mut alone = 0usize;
    for candidate in &candidates {
        if validates_alone(source, source_type, *candidate) {
            alone += 1;
            for byte in &mut emptied[candidate.statements_start as usize..candidate.statements_end as usize] {
                if !matches!(*byte, b'\n' | b'\r') {
                    *byte = b' ';
                }
            }
        }
    }
    let emptied = String::from_utf8(emptied).expect("emptied bodies hold only ASCII");
    let whole = passes_early_checks(source, source_type);
    let verdict = match (whole, passes_early_checks(&emptied, source_type)) {
        (true, true) | (false, false) => "same",
        (false, true) => "missed",
        (true, false) => "introduced",
    };
    eprintln!(
        "lazy-bodies {verdict} source={name} candidates={} alone={alone} whole_ok={whole}",
        candidates.len()
    );
}

/// The environment switch that enables lazy function bodies while they are qualified.
const LAZY_FUNCTION_BODIES_ENV: &str = "QUENCH_LAZY_FUNCTION_BODIES";

/// Whether a program compiles with lazy function bodies.
pub(super) fn enabled() -> bool {
    std::env::var_os(LAZY_FUNCTION_BODIES_ENV).is_some()
}

/// The name a stand-in body calls (see the module documentation).
const STAND_IN_CALLEE: &str = "eval";

/// `source` parsed as a compilation parses it, with the bodies of functions starting at or
/// after `lazy_from` parsed lazily when set.
pub(super) fn parse<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
    lazy_from: Option<u32>,
) -> oxc_parser::ParserReturn<'a> {
    let mut parsed = super::annex_b_targets::parse_program_with(allocator, source, source_type, lazy_from);
    if !parsed.lazy_function_bodies.is_empty() {
        let starts: FxHashSet<u32> = parsed.lazy_function_bodies.iter().copied().collect();
        stand_in(allocator, &mut parsed.program, &starts);
    }
    parsed
}

/// Replaces the statements of each function body of `program` starting at one of `starts`
/// with the stand-in (see the module documentation).
pub(super) fn stand_in<'a>(allocator: &'a Allocator, program: &mut Program<'a>, starts: &FxHashSet<u32>) {
    StandIns { allocator, starts }.visit_program(program);
}

/// Gives each lazily parsed body its stand-in.
struct StandIns<'s, 'a> {
    allocator: &'a Allocator,
    starts: &'s FxHashSet<u32>,
}

impl<'a> VisitMut<'a> for StandIns<'_, 'a> {
    fn visit_function_body(&mut self, body: &mut FunctionBody<'a>) {
        if self.starts.contains(&body.span.start) {
            let builder = oxc_ast::builder::AstBuilder::new(self.allocator);
            let span = oxc_span::Span::empty(body.span.start);
            let callee = Expression::Identifier(IdentifierReference::boxed(span, STAND_IN_CALLEE, &builder));
            let call = CallExpression::boxed(
                span,
                callee,
                None,
                oxc_allocator::Vec::<Argument<'a>>::new_in(&self.allocator),
                false,
                &builder,
            );
            let statement = ExpressionStatement::boxed(span, Expression::CallExpression(call), &builder);
            body.statements.clear();
            body.statements.push(Statement::ExpressionStatement(statement));
            return;
        }
        walk_mut::walk_function_body(self, body);
    }
}

/// Arena bytes reserved per source byte for a parse: the whole program's parse reserves as
/// much (`Compiler::specialize_with_mode_and_private_names`), and pages it does not touch
/// cost no memory.
const ARENA_BYTES_PER_SOURCE_BYTE: usize = 6;

/// The arena for parsing `text` again.
pub(super) fn arena(text: &str) -> Allocator {
    Allocator::with_capacity(text.len().saturating_mul(ARENA_BYTES_PER_SOURCE_BYTE))
}

/// A function's text parsed again alone: its tree and what the whole program's analysis
/// would have found inside it, with every position an offset into the program's text.
pub(super) struct Reparsed<'a> {
    pub(super) program: Program<'a>,
    /// The starts of the function bodies inside it parsed lazily.
    pub(super) lazy_bodies: Vec<u32>,
    /// Its private names, numbered from zero within it (`private_name_ids`).
    pub(super) private_name_ids: rustc_hash::FxHashMap<(u32, u32), u32>,
    pub(super) private_name_labels: rustc_hash::FxHashMap<(u32, u32), String>,
    /// Its function expressions whose `prototype` no program text observes (`call_only`).
    pub(super) prototype_unobservable: FxHashSet<u32>,
}

impl<'a> Reparsed<'a> {
    /// The function parsed again.
    pub(super) fn function(&self) -> Option<&Function<'a>> {
        match self.program.body.first()? {
            Statement::FunctionDeclaration(function) => Some(function),
            Statement::ExpressionStatement(statement) => match statement.expression.without_parentheses() {
                Expression::FunctionExpression(function) => Some(function),
                _ => None,
            },
            _ => None,
        }
    }
}

/// `text`, a function's source at `offset` wrapped for the context it is compiled in, parsed
/// again with the bodies inside it parsed lazily, when it passes every early check a
/// compilation applies; positions move by `delta` into the program's text.
pub(super) fn reparse<'a>(
    allocator: &'a Allocator,
    text: &'a str,
    offset: u32,
    delta: u32,
    source_type: SourceType,
    strict: bool,
) -> Option<Reparsed<'a>> {
    let mut parsed = parse(allocator, text, source_type, Some(offset + 1));
    if parsed.stack_exhausted
        || !parsed.diagnostics.is_empty()
        || super::stack::validate(&parsed.program).is_err()
        || super::early::regexp_early_error(&parsed.program).is_some()
        || super::annex_b_targets::invalid_target(&parsed.program).is_some()
        || !super::early::program_early_errors(&parsed.program, strict).1.is_empty()
    {
        return None;
    }
    let built = oxc_semantic::SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !built.diagnostics.is_empty() {
        return None;
    }
    let shift = |(start, end): (u32, u32)| (start.wrapping_add(delta), end.wrapping_add(delta));
    let private_name_ids = super::private_name_ids(&built.semantic)
        .into_iter()
        .map(|(span, id)| (shift(span), id))
        .collect();
    let private_name_labels = super::private_name_labels(&built.semantic)
        .into_iter()
        .map(|(span, label)| (shift(span), label))
        .collect();
    let prototype_unobservable = super::call_only::prototype_unobservable_functions(&built.semantic, &parsed.program)
        .into_iter()
        .map(|start| start.wrapping_add(delta))
        .collect();
    drop(built);
    oxc_ast_visit::VisitMut::visit_program(&mut super::replay::ShiftSpans(delta), &mut parsed.program);
    Some(Reparsed {
        lazy_bodies: parsed.lazy_function_bodies.iter().map(|start| start.wrapping_add(delta)).collect(),
        program: parsed.program,
        private_name_ids,
        private_name_labels,
        prototype_unobservable,
    })
}

/// A function's source wrapped to parse alone in its context: `wrapper` gives it the
/// syntactic form it had, and a directive the strictness it inherits where the goal does
/// not imply it. Returns the text and the offset of the function's source in it.
pub(super) fn text_alone(
    snippet: &str,
    (prefix, suffix): (&str, &str),
    inherited_strict: bool,
    source_type: SourceType,
) -> (String, u32) {
    let strict = if inherited_strict && !source_type.is_module() { "\"use strict\";" } else { "" };
    let text = format!("{strict}{prefix}{snippet}{suffix}");
    (text, (strict.len() + prefix.len()) as u32)
}

/// The wrapper of a plain function parsed alone: an expression parses parenthesized.
fn plain_wrapper(expression: bool) -> (&'static str, &'static str) {
    if expression { ("(", ")") } else { ("", "") }
}

/// Whether `source` parses as `source_type` with no syntax error OXC reports, parsing and
/// checking the bodies of its functions one at a time: a lazily parsed body is checked as
/// its source parsed alone, which reports every error the whole program reports inside it
/// (TOD-929 stage 1), so the whole program's tree never exists at once. A body can fail
/// alone and not in its program (a private name of an enclosing class), so a failure inside
/// one is settled by checking the whole program.
pub(crate) fn syntax_valid(source: &str, source_type: SourceType) -> bool {
    match syntax_verdict(source, source_type, 0) {
        Verdict::Valid => true,
        Verdict::Invalid => false,
        Verdict::BodyFailed => syntax_valid_whole(source, source_type),
    }
}

/// The whole program's tree checked at once.
fn syntax_valid_whole(source: &str, source_type: SourceType) -> bool {
    let allocator = arena(source);
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    parsed.diagnostics.is_empty()
        && oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .is_empty()
}

/// What a lazily parsed check of a text found.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Valid,
    /// The code outside its lazily parsed bodies has an error, which the whole text has too.
    Invalid,
    /// A body parsed alone has an error, which the whole text may not.
    BodyFailed,
}

fn syntax_verdict(text: &str, source_type: SourceType, lazy_from: u32) -> Verdict {
    let allocator = arena(text);
    let parsed = oxc_parser::Parser::new(&allocator, text, source_type)
        .with_options(oxc_parser::ParseOptions {
            lazy_function_bodies: Some(lazy_from),
            ..oxc_parser::ParseOptions::default()
        })
        .parse();
    if !parsed.diagnostics.is_empty()
        || !oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .is_empty()
    {
        return Verdict::Invalid;
    }
    if parsed.lazy_function_bodies.is_empty() {
        return Verdict::Valid;
    }
    let mut lazy = LazyFunctions {
        starts: parsed.lazy_function_bodies.iter().copied().collect(),
        strict: source_type.is_module() || has_use_strict(&parsed.program.directives),
        found: Vec::new(),
    };
    oxc_ast_visit::Visit::visit_program(&mut lazy, &parsed.program);
    let found = lazy.found;
    drop(parsed);
    drop(allocator);
    let body_valid = |function: LazyFunction| {
        let snippet = &text[function.start as usize..function.end as usize];
        let (alone, offset) = text_alone(snippet, plain_wrapper(function.expression), function.inherited_strict, source_type);
        syntax_verdict(&alone, source_type, offset + 1) == Verdict::Valid
    };
    if found.into_iter().all(body_valid) { Verdict::Valid } else { Verdict::BodyFailed }
}

fn has_use_strict(directives: &[oxc_ast::ast::Directive<'_>]) -> bool {
    directives.iter().any(|directive| directive.directive == "use strict")
}

/// A function whose body was parsed lazily, as it parses alone.
struct LazyFunction {
    start: u32,
    end: u32,
    expression: bool,
    inherited_strict: bool,
}

/// The functions of a program whose bodies were parsed lazily, with the strictness each
/// inherits.
struct LazyFunctions {
    starts: FxHashSet<u32>,
    strict: bool,
    found: Vec<LazyFunction>,
}

impl<'a> oxc_ast_visit::Visit<'a> for LazyFunctions {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        let Some(body) = function.body.as_deref() else {
            return oxc_ast_visit::walk::walk_function(self, function, flags);
        };
        if self.starts.contains(&body.span.start) {
            self.found.push(LazyFunction {
                start: function.span.start,
                end: function.span.end,
                expression: function.r#type == oxc_ast::ast::FunctionType::FunctionExpression,
                inherited_strict: self.strict,
            });
            return;
        }
        let strict = self.strict;
        self.strict |= has_use_strict(&body.directives);
        oxc_ast_visit::walk::walk_function(self, function, flags);
        self.strict = strict;
    }

    fn visit_class(&mut self, class: &oxc_ast::ast::Class<'a>) {
        let strict = std::mem::replace(&mut self.strict, true);
        oxc_ast_visit::walk::walk_class(self, class);
        self.strict = strict;
    }
}

#[cfg(test)]
mod tests {
    use super::{syntax_valid, syntax_valid_whole as valid_whole};
    use oxc_span::SourceType;

    #[test]
    fn lazy_syntax_check_agrees_with_the_whole_program() {
        let sources = [
            "function f() { return 1 }",
            "function f() { let a; let a; }",
            "function f() { var x = 010; }",
            "'use strict'; function f() { var x = 010; }",
            "function f() { 'use strict'; with (a) {} }",
            "class C { m() { function g() { with (a) {} } } }",
            "var g = function () { return arguments }; function h(a, a) {}",
            "'use strict'; function h(a, a) {}",
            "function f() { function g() { break; } }",
            "function f() { yield: 1 } function* g() { function h() { var yield; } }",
            "(function () { function f() { return } })()",
            "function f() { import.meta }",
            "function f() { return \"\\u0061\" } let s = '\\u0062'",
            "function f() { await: 1 }",
            "class C { #p = 1; m() { return function () { return this.#p } } }",
            "class C { m() { return function () { return this.#q } } }",
        ];
        for source in sources {
            for source_type in [SourceType::cjs(), SourceType::mjs(), SourceType::script()] {
                assert_eq!(
                    syntax_valid(source, source_type),
                    valid_whole(source, source_type),
                    "{source_type:?}: {source}"
                );
            }
        }
    }

    /// Compares the two checks on every `.js`, `.cjs` and `.mjs` file under
    /// `QUENCH_SYNTAX_CORPUS`, as scripts, CommonJS and modules.
    #[test]
    #[ignore = "reads a corpus named by QUENCH_SYNTAX_CORPUS"]
    fn lazy_syntax_check_agrees_on_a_corpus() {
        let root = std::env::var_os("QUENCH_SYNTAX_CORPUS").expect("QUENCH_SYNTAX_CORPUS");
        let mut pending = vec![std::path::PathBuf::from(root)];
        let (mut files, mut differ) = (0usize, Vec::new());
        while let Some(path) = pending.pop() {
            if path.is_dir() {
                pending.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
                continue;
            }
            if !path.extension().is_some_and(|extension| matches!(extension.to_str(), Some("js" | "cjs" | "mjs"))) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else { continue };
            files += 1;
            for source_type in [SourceType::cjs(), SourceType::mjs()] {
                if syntax_valid(&source, source_type) != valid_whole(&source, source_type) {
                    differ.push(format!("{source_type:?} {}", path.display()));
                }
            }
        }
        eprintln!("files={files} differ={}", differ.len());
        assert!(differ.is_empty(), "{differ:#?}");
    }
}
