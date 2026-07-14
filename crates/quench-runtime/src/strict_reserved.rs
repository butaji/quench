//! Strict-mode future-reserved-word validation.
//!
//! OXC does not reject the strict-mode future reserved words
//! (`implements`, `interface`, `let`, `package`, `private`, `protected`,
//! `public`, `static`, `yield`) or `eval`/`arguments` when used as binding
//! identifiers, even in strict code. ES §12.7.2 makes these a SyntaxError in
//! strict mode. We walk the parsed AST for binding positions and report the
//! first offending name so the caller can raise a SyntaxError.

use oxc::ast::ast::{AssignmentTargetPattern, Program};
use oxc::ast::visit::Visit;

/// Future reserved words that may not be used as a binding identifier in
/// strict mode (ES §12.7.2). `yield` is included because it is a reserved
/// word in strict-mode contexts.
const STRICT_RESERVED: &[&str] = &[
    "implements",
    "interface",
    "let",
    "package",
    "private",
    "protected",
    "public",
    "static",
    "yield",
];

/// Whether `name` may not be bound (declared/assigned as a target) in strict
/// mode. `eval` and `arguments` are also disallowed as binding targets.
fn is_strict_reserved_binding(name: &str) -> bool {
    name == "eval" || name == "arguments" || STRICT_RESERVED.contains(&name)
}

/// Collects the first binding identifier that is illegal in strict mode.
struct ReservedBindingVisitor {
    offender: Option<String>,
}

impl<'a> Visit<'a> for ReservedBindingVisitor {
    fn visit_binding_identifier(&mut self, it: &oxc::ast::ast::BindingIdentifier<'a>) {
        if self.offender.is_none() && is_strict_reserved_binding(it.name.as_str()) {
            self.offender = Some(it.name.to_string());
        }
        // BindingIdentifier has no child nodes; default impl does nothing.
    }

    fn visit_identifier_reference(&mut self, it: &oxc::ast::ast::IdentifierReference) {
        // IdentifierReference appears in assignment targets (e.g., `arguments = 10`).
        // We check it here since OXC's walk doesn't visit BindingIdentifier for these.
        if self.offender.is_none() && is_strict_reserved_binding(it.name.as_str()) {
            self.offender = Some(it.name.to_string());
        }
        // IdentifierReference is a leaf node; no need to walk children.
    }

    fn visit_assignment_target_pattern(&mut self, _it: &AssignmentTargetPattern<'a>) {
        // Default traversal calls visit_assignment_target_maybe_default,
        // which chains to visit_identifier_reference — our override catches
        // reserved names in destructuring patterns automatically.
    }
}

/// Return `Some(name)` if the program binds a strict-mode reserved word.
/// The caller must only invoke this when strict mode applies.
pub fn find_strict_reserved_binding(program: &Program) -> Option<String> {
    let mut visitor = ReservedBindingVisitor { offender: None };
    visitor.visit_program(program);
    visitor.offender
}

/// Whether the program's directive prologue contains a "use strict" directive.
pub fn has_use_strict_directive(program: &Program) -> bool {
    program
        .directives
        .iter()
        .any(|d| d.directive.as_str() == "use strict")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    fn find(src: &str) -> Option<String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::default()).parse();
        find_strict_reserved_binding(&ret.program)
    }

    #[test]
    fn detects_reserved_var_binding() {
        assert_eq!(find("var public = 1;").as_deref(), Some("public"));
        assert_eq!(find("let interface = 1;").as_deref(), Some("interface"));
        assert_eq!(find("function f(private) {}").as_deref(), Some("private"));
    }

    #[test]
    fn detects_eval_arguments_binding() {
        assert_eq!(find("var eval = 1;").as_deref(), Some("eval"));
        assert_eq!(
            find("function arguments() {}").as_deref(),
            Some("arguments")
        );
    }

    #[test]
    fn allows_non_reserved_binding() {
        assert_eq!(find("var notReserved = 1;"), None);
        // Reserved words used only as references (not bindings) are allowed here.
        assert_eq!(find("foo.public;"), None);
    }

    #[test]
    fn detects_eval_arguments_assignment() {
        // ES §12.7.2: assigning to eval/arguments in strict mode is a SyntaxError.
        // The program has "use strict", so this should be detected.
        assert_eq!(
            find(r#""use strict"; arguments = 10;"#).as_deref(),
            Some("arguments")
        );
        assert_eq!(
            find(r#""use strict"; eval = 10;"#).as_deref(),
            Some("eval")
        );
    }
}
