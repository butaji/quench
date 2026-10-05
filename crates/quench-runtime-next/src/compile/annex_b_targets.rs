//! OXC owns call-target recognition. Its opt-in AST representation preserves
//! original spans; the compiler validates and lowers these targets as Annex B
//! runtime errors without rewriting guest source.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentExpression, AssignmentTargetMaybeDefault, AssignmentTargetRest, Expression,
    SimpleAssignmentTarget,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{CALL_ASSIGNMENT_TARGET_MARKER, ParseOptions, Parser, ParserReturn};
use oxc_span::{GetSpan, SourceType, Span};

pub(super) fn parse_program<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> ParserReturn<'a> {
    Parser::new(allocator, source, source_type)
        .with_options(ParseOptions {
            allow_call_assignment_targets: true,
            ..ParseOptions::default()
        })
        .parse()
}

pub(super) fn call_target<'s, 'a>(
    target: &'s SimpleAssignmentTarget<'a>,
) -> Option<&'s Expression<'a>> {
    match target {
        SimpleAssignmentTarget::StaticMemberExpression(member)
            if member.property.name == CALL_ASSIGNMENT_TARGET_MARKER =>
        {
            Some(&member.object)
        }
        _ => None,
    }
}

/// Logical assignments and destructuring patterns require simple targets even
/// in non-strict Annex B code. Calls inside keys/default expressions remain valid.
pub(super) fn invalid_target(program: &oxc_ast::ast::Program<'_>) -> Option<Span> {
    struct Targets {
        invalid: Option<Span>,
    }
    impl<'a> Visit<'a> for Targets {
        fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
            if assignment.operator.is_logical()
                && assignment
                    .left
                    .as_simple_assignment_target()
                    .and_then(call_target)
                    .is_some()
            {
                self.invalid.get_or_insert(assignment.left.span());
            }
            walk::walk_assignment_expression(self, assignment);
        }

        fn visit_assignment_target_maybe_default(
            &mut self,
            target: &AssignmentTargetMaybeDefault<'a>,
        ) {
            let binding = match target {
                AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                    Some(&default.binding)
                }
                _ => target.as_assignment_target(),
            };
            if let Some(call) = binding
                .and_then(|binding| binding.as_simple_assignment_target())
                .and_then(call_target)
            {
                self.invalid.get_or_insert(call.span());
            }
            walk::walk_assignment_target_maybe_default(self, target);
        }

        fn visit_assignment_target_rest(&mut self, rest: &AssignmentTargetRest<'a>) {
            if let Some(call) = rest
                .target
                .as_simple_assignment_target()
                .and_then(call_target)
            {
                self.invalid.get_or_insert(call.span());
            }
            walk::walk_assignment_target_rest(self, rest);
        }
    }
    let mut targets = Targets { invalid: None };
    targets.visit_program(program);
    targets.invalid
}
