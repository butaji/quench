//! A class's method, getter or setter whose body the parser skimmed and released
//! (`ParseOptions::lazy_class_methods`): a first-call stub where its free names are known and
//! its place lets one compile, else compiled from its text parsed again alone, in a class.
//! The skim decides no `super`, so a released method's body names none and needs no home.
use super::{Nesting, is_stand_in, reparse, text_alone};
use crate::bytecode::Atom;
use crate::compile::{Compiler, capture_scope::ScopeChain, class, first_call, replay};
use oxc_ast::ast::MethodDefinition;
use oxc_span::{GetSpan, Span};

/// Whether the body of `method` is a released body's stand-in.
pub(in crate::compile) fn is_released_method(method: &MethodDefinition<'_>) -> bool {
    method.value.body.as_ref().is_some_and(|body| is_stand_in(body))
}

/// Where a class's method compiles: what `Compiler::compile_class_method` takes besides it.
pub(in crate::compile) struct MethodPlace<'s> {
    /// The class element's span, which includes `static`.
    pub(in crate::compile) source_span: Option<Span>,
    pub(in crate::compile) scopes: &'s ScopeChain,
    pub(in crate::compile) parent: Option<u32>,
    pub(in crate::compile) home_atom: Atom,
    pub(in crate::compile) with_depth: u16,
}

impl Compiler<'_> {
    /// `method`, whose body was released (`is_released_method`): its first-call stub, or the
    /// method compiled from its text parsed again. A text that fails alone fails the
    /// compilation, which then compiles the program eagerly.
    pub(in crate::compile) fn compile_released_method(&mut self, method: &MethodDefinition<'_>, place: MethodPlace<'_>) -> u32 {
        if let Some(id) = self.skip_replayed_child() {
            return id;
        }
        let span = place.source_span.and_then(|span| class::class_method_source_span(self.text, span, method.key.span().start));
        if let Some(span) = span
            && let Some(captures) = self.nested_method_stub_captures(method, &place)
        {
            let id = self.compile_first_call_method_stub(method, span, &place);
            self.note_nested_stub(id, captures);
            return id;
        }
        if let Some(body) = &method.value.body {
            self.lazy_compiled.push(body.span.start);
        }
        let Some(span) = span else {
            return self.reject_released_method(method, place);
        };
        let snippet = &self.text[span.start as usize..span.end as usize];
        let (text, offset) = text_alone(snippet, replay::CLASS_METHOD_WRAPPER, false, self.source_type);
        // Bodies parse lazily past the method's own start, so its own parses whole again.
        let own = offset + (method.value.span.start - span.start);
        let mut allocator = self.lazy_arenas.pop().unwrap_or_else(|| super::arena(&text));
        let saved_names = self.body_type_names();
        let nesting = if self.replay_target.is_some() { Nesting::Recorded(&self.lazy_bodies) } else { Nesting::Parse(&mut self.first_call.free_names, self.first_call.direct) };
        let mut reparsed = reparse(&allocator, &text, own, span.start.wrapping_sub(offset), self.source_type, nesting, saved_names);
        // The method's declarations join the program's, and it resolves its annotations against the names they were admitted with.
        let names = match reparsed.as_mut() {
            Ok(reparsed) => {
                let declared = std::mem::take(&mut reparsed.declared);
                let names = declared.type_names.clone();
                self.declared_types.append(declared);
                names
            }
            Err(_) => None,
        };
        let again = reparsed.as_ref().ok().and_then(|reparsed| replay::wrapped_class_method(&reparsed.program)).filter(|again| again.value.span == method.value.span && !is_released_method(again));
        let id = match again {
            Some(again) => self.with_type_names(names, |this| this.compile_class_method(again, place.source_span, place.scopes, place.parent, None, None, false, place.home_atom, place.with_depth)),
            None => self.reject_released_method(method, place),
        };
        drop(reparsed);
        super::release_after(&mut allocator);
        self.lazy_arenas.push(allocator);
        id
    }

    /// The stand-in of `method`'s body compiled in its place, failing the compilation.
    fn reject_released_method(&mut self, method: &MethodDefinition<'_>, place: MethodPlace<'_>) -> u32 {
        self.reject(method.span, "SyntaxError: a lazily parsed method body does not compile alone");
        let body = method.value.body.as_ref().map_or(&[][..], |body| body.statements.as_slice());
        let params = crate::compile::FunctionCompiler::params_from_formals(&method.value.params, self);
        let options = crate::compile::FunctionOptions { strict: true, non_constructible: true, with_depth: place.with_depth, ..Default::default() };
        self.compile_function(None, &params, crate::compile::FunctionBody::Statements(body), place.scopes, place.parent, options)
    }

    /// `method`, a class's method, getter or setter whose body was parsed lazily, compiled as
    /// its first-call stub at `span` (from its key, past `static`): a strict function of its
    /// name and length that constructs nothing, whose first call forwards to its chunk.
    fn compile_first_call_method_stub(&mut self, method: &MethodDefinition<'_>, span: Span, place: &MethodPlace<'_>) -> u32 {
        self.first_call.room.add_stub(span.end - span.start);
        let name = class::class_method_function_name(method);
        let source_span = self.source_span(span);
        let own = first_call::StubOwn { body: first_call::ImplicitBody::FirstCallArrowStub, strict: true, name: name.as_deref(), parent: place.parent, source_span, constructible: false };
        let id = self.templated_stub(own, |compiler| compiler.compile_function(
            name.as_deref(),
            &[first_call::STUB_ARGUMENTS.to_owned()],
            crate::compile::FunctionBody::Statements(&[]),
            place.scopes,
            place.parent,
            crate::compile::FunctionOptions {
                source_span,
                rest_override: true,
                implicit_body: Some(first_call::ImplicitBody::FirstCallArrowStub),
                non_constructible: true,
                strict: true,
                ..Default::default()
            },
        ));
        if let Some(stub) = self.functions.get_mut(id) {
            stub.length = Self::formal_length(&method.value.params) as u16;
        }
        id
    }
}
