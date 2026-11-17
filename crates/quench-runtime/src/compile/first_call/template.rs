//! First-call stubs copied from a template. Every stub of one implicit body and strictness
//! has the same code: it gathers its arguments, reads `new.target` unless it has none, and
//! calls its chunk. The first such stub of a compilation compiles through `compile_function`;
//! the rest copy its record with their own parent, name and source, which skips the whole
//! front end per stub (babylonjs-startup makes some 38,000 of them).
use super::ImplicitBody;
use crate::bytecode::{Function as BcFunction, SourceSpan};
use crate::compile::Compiler;

/// The stub records a compilation copies, by implicit body and strictness.
#[derive(Default)]
pub(in crate::compile) struct StubTemplates([Option<BcFunction>; 4]);

/// What a stub's record holds of its own: everything else is its template's.
pub(in crate::compile) struct StubOwn<'n> {
    pub(in crate::compile) body: ImplicitBody,
    pub(in crate::compile) strict: bool,
    pub(in crate::compile) name: Option<&'n str>,
    pub(in crate::compile) parent: Option<u32>,
    pub(in crate::compile) source_span: Option<SourceSpan>,
    /// Whether `new` may call it, as `compile_function` gives it before its caller marks it.
    pub(in crate::compile) constructible: bool,
}

fn slot(body: ImplicitBody, strict: bool) -> Option<usize> {
    let kind = match body {
        ImplicitBody::FirstCallStub => 0,
        ImplicitBody::FirstCallArrowStub => 2,
        ImplicitBody::Super => return None,
    };
    Some(kind + usize::from(strict))
}

impl Compiler<'_> {
    /// The stub `own` describes: a copy of its template when the compilation has one and the
    /// stub compiles as the template did, otherwise `compile`'s, kept as the template.
    pub(in crate::compile) fn templated_stub(&mut self, own: StubOwn<'_>, compile: impl FnOnce(&mut Self) -> u32) -> u32 {
        let strict = self.root_strict || own.strict;
        let slot = slot(own.body, strict).filter(|_| self.stub_copies_apply(own.parent, own.source_span));
        let Some(slot) = slot else { return compile(self) };
        if let Some(template) = &self.first_call.templates.0[slot] {
            let mut function = template.clone();
            function.parent = own.parent;
            function.name = own.name.map(|name| self.atom(name));
            function.source_span = own.source_span;
            function.constructible = own.constructible;
            let id = self.functions.reserve();
            self.functions.set(id, function);
            self.record_function_exit(id);
            return id;
        }
        let id = compile(self);
        let template = self.functions.get(id).filter(|function| function.source_positions.last().is_none() && function.strict == strict);
        self.first_call.templates.0[slot] = template.cloned();
        id
    }

    /// Whether a stub's record depends on nothing a template does not share: it nests in a
    /// function, the compilation replays nothing, which places its functions by the load's
    /// cursors, and the stub is no streamed body's run (`later_run`).
    fn stub_copies_apply(&self, parent: Option<u32>, span: Option<SourceSpan>) -> bool {
        parent.is_some() && self.replay_target.is_none() && self.body_stream.is_none() && self.later_run(parent, span).is_none()
    }
}
