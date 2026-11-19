//! OXC owns call-target recognition. Its opt-in AST representation preserves
//! original spans; the compiler validates and lowers these targets as Annex B
//! runtime errors without rewriting guest source.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Program, SimpleAssignmentTarget};
use oxc_parser::{CALL_ASSIGNMENT_TARGET_MARKER, ParseOptions, Parser, ParserReturn};
use oxc_span::{SourceType, Span};

use super::checks::{Passes, walk};

pub(super) fn parse_program<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> ParserReturn<'a> {
    parse_program_with(allocator, source, source_type, None, None, None, false, false, Recognize::No, None, None)
}

/// Which lazily parsed bodies are recognized without their trees (`ParseOptions::lazy_recognize`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Recognize {
    No,
    /// Every body.
    All,
    /// Bodies with no function inside (`ParseOptions::lazy_recognize_flat`), the only ones a
    /// check whose call-only facts are not deferred takes without the tree.
    Flat,
}

/// `parse_program`, with the function bodies starting at or after `lazy_from` parsed lazily
/// (`lazy_bodies`), each function checked by `lazy_body_check` before its body is released.
pub(super) fn parse_program_with<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
    lazy_from: Option<u32>,
    lazy_body_check: Option<oxc_parser::LazyBodyCheck>,
    lazy_body_skim: Option<oxc_parser::LazyBodySkim>,
    inherited_strict: bool,
    nested: bool,
    recognize: Recognize,
    lazy_body_skip: Option<oxc_parser::LazyBodySkip>,
    declared: Option<&mut crate::bytecode::DeclaredTypes>,
) -> ParserReturn<'a> {
    let flat = recognize == Recognize::Flat;
    let recognize = recognize != Recognize::No && lazy_body_check.is_some() && crate::trace::Dev::Recognize.value() != Some("off");
    let mut parsed = Parser::new(allocator, source, source_type)
        .with_options(ParseOptions {
            allow_call_assignment_targets: true,
            lazy_function_bodies: lazy_from,
            lazy_nested_bodies: nested && lazy_body_check.is_some(),
            lazy_body_check,
            lazy_class_methods: lazy_body_skim.is_some(),
            lazy_body_skim,
            inherited_strict,
            lazy_recognize: recognize,
            lazy_recognize_verify: recognize && crate::trace::Dev::Recognize.value() == Some("verify"),
            lazy_recognize_flat: flat,
            lazy_body_skip,
            ..ParseOptions::default()
        })
        .parse();
    // Every compile parse of typed text admits it here, so a body compiled later (lazily,
    // streamed, replayed) is the tree its load admitted.
    if source_type.is_typescript() && parsed.diagnostics.is_empty() && !parsed.stack_exhausted {
        let mut discarded = crate::bytecode::DeclaredTypes::default();
        super::typescript::admit(allocator, &mut parsed.program, declared.unwrap_or(&mut discarded));
    }
    parsed
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
/// `invalid_target` of a parse, which only a call assignment target can fail
/// (`ParserReturn::has_call_assignment_targets`).
pub(super) fn invalid_parsed_target(parsed: &ParserReturn<'_>) -> Option<Span> {
    parsed.has_call_assignment_targets.then(|| invalid_target(&parsed.program)).flatten()
}

/// The first call assignment target Annex B rejects in `program`.
pub(super) fn invalid_target(program: &Program<'_>) -> Option<Span> {
    walk(program, Passes { targets: true, ..Passes::default() }).invalid_target
}
