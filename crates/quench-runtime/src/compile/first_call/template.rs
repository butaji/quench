//! First-call stubs copied from a template. Every stub of one implicit body and strictness
//! has the same code: it gathers its arguments, reads `new.target` unless it has none, and
//! calls its chunk. The first such stub of a compilation compiles through `compile_function`;
//! the rest copy its record with their own parent, name and source, which skips the whole
//! front end per stub (babylonjs-startup makes some 38,000 of them).
use super::ImplicitBody;
use crate::bytecode::{Function as BcFunction, SourceSpan};
use crate::compile::Compiler;

/// Template slots per implicit body: one sloppy, one strict.
const STRICTNESS_VARIANTS: usize = 2;
/// The implicit bodies a stub can be copied from: a function's and an arrow's.
const STUB_BODIES: usize = 2;
const SLOTS: usize = STUB_BODIES * STRICTNESS_VARIANTS;

/// The stub records a compilation copies, by implicit body and strictness, and the slot of
/// each stub made from one, its template's own included, which the finished program's
/// slots share one record for (`unlowered`).
#[derive(Default)]
pub(in crate::compile) struct StubTemplates {
    records: [Option<BcFunction>; SLOTS],
    /// The template slot of each templated stub, indexed by function id.
    kinds: Vec<Option<u8>>,
    /// The record the finished program's stubs of each slot share.
    shared: [Option<std::rc::Rc<BcFunction>>; SLOTS],
}

impl StubTemplates {
    fn mark(&mut self, id: u32, slot: usize) {
        let id = id as usize;
        if self.kinds.len() <= id {
            self.kinds.resize(id + 1, None);
        }
        self.kinds[id] = u8::try_from(slot).ok();
    }

    /// The finished program's slot of function `id`, unlowered: a templated stub shares the
    /// first such record of its slot, which its header makes its own on first read
    /// (`FunctionSlot::unlowered_template`); any other function is packed.
    pub(in crate::compile) fn unlowered(&mut self, id: usize, function: BcFunction) -> crate::bytecode::FunctionSlot {
        let Some(kind) = self.kinds.get(id).copied().flatten() else {
            return crate::bytecode::FunctionSlot::unlowered_packed(function);
        };
        let template = self.shared[usize::from(kind)].get_or_insert_with(|| std::rc::Rc::new(function.clone())).clone();
        crate::bytecode::FunctionSlot::unlowered_template(&function, template)
    }
}

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
    let body = match body {
        ImplicitBody::FirstCallStub => 0,
        ImplicitBody::FirstCallArrowStub => 1,
        ImplicitBody::Super => return None,
    };
    Some(body * STRICTNESS_VARIANTS + usize::from(strict))
}

impl Compiler<'_> {
    /// The stub `own` describes: a copy of its template when the compilation has one and the
    /// stub compiles as the template did, otherwise `compile`'s, kept as the template.
    pub(in crate::compile) fn templated_stub(&mut self, own: StubOwn<'_>, compile: impl FnOnce(&mut Self) -> u32) -> u32 {
        let strict = self.root_strict || own.strict;
        let slot = slot(own.body, strict).filter(|_| self.stub_copies_apply(own.parent, own.source_span));
        let Some(slot) = slot else { return compile(self) };
        if let Some(template) = &self.first_call.templates.records[slot] {
            let mut function = template.clone();
            function.parent = own.parent;
            function.name = own.name.map(|name| self.atom(name));
            function.source_span = own.source_span;
            function.constructible = own.constructible;
            let id = self.functions.reserve();
            self.functions.set(id, function);
            self.record_function_exit(id);
            self.first_call.templates.mark(id, slot);
            return id;
        }
        let id = compile(self);
        let template = self.functions.get(id).filter(|function| function.source_positions.last().is_none() && function.strict == strict);
        self.first_call.templates.records[slot] = template.cloned();
        if template.is_some() {
            self.first_call.templates.mark(id, slot);
        }
        id
    }

    /// Whether a stub's record depends on nothing a template does not share: it nests in a
    /// function, the compilation replays nothing, which places its functions by the load's
    /// cursors, and the stub is no streamed body's run (`later_run`).
    fn stub_copies_apply(&self, parent: Option<u32>, span: Option<SourceSpan>) -> bool {
        parent.is_some() && self.replay_target.is_none() && self.body_stream.is_none() && self.later_run(parent, span).is_none()
    }
}
