//! An arrow function whose block body the parser released, compiled from its source parsed
//! again alone (`lazy_bodies`), in the wrapper that gives it the `this`, `super` and
//! `new.target` it had (`replay::arrow_wrapper`): the skim leaves a body that names any of
//! them to the whole parse, so what the arrow reads of them is what its wrapper gives it.
use super::{Nesting, is_stand_in, reparse, text_alone};
use crate::compile::{Compiler, replay};
use oxc_ast::ast::{ArrowFunctionBody, ArrowFunctionExpression};

/// Whether the block body of `arrow` is a released body's stand-in.
pub(in crate::compile) fn is_released_arrow(arrow: &ArrowFunctionExpression<'_>) -> bool {
    matches!(&arrow.body, ArrowFunctionBody::FunctionBody(body) if is_stand_in(body))
}

impl Compiler<'_> {
    /// `arrow`, whose body was released (`is_released_arrow`), compiled from its source parsed
    /// again. A body that fails an early check alone fails the compilation, which then
    /// compiles the program eagerly; until then its stand-in compiles in its place.
    pub(in crate::compile) fn compile_released_arrow(&mut self, arrow: &ArrowFunctionExpression<'_>, context: &replay::ArrowContext) -> u32 {
        if let Some(id) = self.skip_replayed_child() {
            return id;
        }
        if let Some(captures) = self.nested_arrow_stub_captures(arrow, context) {
            let id = self.compile_first_call_arrow_stub(arrow, context);
            self.note_nested_stub(id, captures);
            return id;
        }
        if let ArrowFunctionBody::FunctionBody(body) = &arrow.body {
            self.lazy_compiled.push(body.span.start);
        }
        let snippet = &self.text[arrow.span.start as usize..arrow.span.end as usize];
        let wrapper = replay::arrow_wrapper(context.inherited_strict || self.module_goal);
        let (text, offset) = text_alone(snippet, wrapper, false, self.source_type);
        let mut allocator = self.lazy_arenas.pop().unwrap_or_else(|| super::arena(&text));
        let nesting = if self.replay_target.is_some() { Nesting::Recorded(&self.lazy_bodies) } else { Nesting::Parse(&mut self.first_call.free_names, self.first_call.direct) };
        let reparsed = reparse(&allocator, &text, offset, arrow.span.start.wrapping_sub(offset), self.source_type, nesting);
        let id = match reparsed.as_ref().and_then(|reparsed| reparsed.arrow()).filter(|again| again.span == arrow.span) {
            Some(again) => self.compile_arrow_function(again, context),
            None => {
                self.reject(arrow.span, "SyntaxError: a lazily parsed function body does not compile alone");
                self.compile_arrow_function(arrow, context)
            }
        };
        drop(reparsed);
        super::release_after(&mut allocator);
        self.lazy_arenas.push(allocator);
        id
    }
}
