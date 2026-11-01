//! Literal inputs to host-owned global Script operations. This is conservative
//! source staging, not permission to replace a call or its coercion behavior.

use super::NativeSourceKey;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;

pub(super) fn sources(program: &Program<'_>, scoping: &Scoping, specialized: bool, calls: &[(String, String)]) -> Vec<NativeSourceKey> {
    struct Discovery<'s> {
        scoping: &'s Scoping,
        calls: &'s [(String, String)],
        specialized: bool,
        requests: Vec<NativeSourceKey>,
    }
    impl<'a> Visit<'a> for Discovery<'_> {
        fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
            if let Expression::StaticMemberExpression(member) = call.callee.get_inner_expression()
                && let Expression::Identifier(global) = member.object.get_inner_expression()
                && let Some(reference) = global.reference_id.get()
                && self.scoping.get_reference(reference).symbol_id().is_none()
                && self.calls.iter().any(|(object, method)| object == global.name.as_str() && method == member.property.name.as_str())
                && let Some(argument) = call.arguments.first().and_then(Argument::as_expression)
                && let Expression::StringLiteral(source) = argument.get_inner_expression()
            {
                self.requests.push(NativeSourceKey::global_script(self.specialized, source.value.as_str()));
            }
            walk::walk_call_expression(self, call);
        }
    }
    if calls.is_empty() { return Vec::new(); }
    let mut discovery = Discovery { scoping, calls, specialized, requests: Vec::new() };
    discovery.visit_program(program);
    discovery.requests
}
