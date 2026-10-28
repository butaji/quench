//! Function bodies that can be validated, and later compiled, apart from the program around
//! them (TOD-929). A candidate is a plain function declaration or expression whose body no
//! enclosing candidate holds: its text, parsed alone in the syntactic context `replay`
//! gives it, must report every early error the whole program reports inside that body.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Class, Expression, Function, FunctionType, MethodDefinition, ObjectProperty, ParenthesizedExpression,
    Program, PropertyKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::SourceType;
use oxc_syntax::scope::ScopeFlags;

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

fn has_use_strict(directives: &[oxc_ast::ast::Directive<'_>]) -> bool {
    directives.iter().any(|directive| directive.directive == "use strict")
}

struct Candidates {
    strict: bool,
    /// The next function visited is a method, an accessor or a parenthesized expression.
    not_candidate: bool,
    found: Vec<Candidate>,
}

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
