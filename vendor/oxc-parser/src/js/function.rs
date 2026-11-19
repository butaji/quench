use oxc_allocator::{ArenaBox, ArenaVec, GetAllocator};
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::{FunctionKind, skim::{LazyForm, Skimmed}};
use crate::cursor::ParserCheckpoint;
use crate::{
    Context, ParserConfig as Config, ParserImpl, StatementContext, diagnostics,
    lexer::Kind,
    modifiers::{ModifierKind, ModifierKinds, Modifiers},
};

impl FunctionKind {
    pub(crate) fn is_id_required(self) -> bool {
        matches!(self, Self::Declaration)
    }

    pub(crate) fn is_expression(self) -> bool {
        self == Self::Expression
    }
}

impl<'a, C: Config> ParserImpl<'a, C> {
    pub(crate) fn at_function_with_async(&mut self) -> bool {
        self.at(Kind::Function)
            || self.at(Kind::Async) && {
                let token = self.lexer.peek_token();
                token.kind() == Kind::Function && !token.is_on_new_line()
            }
    }

    pub(crate) fn parse_function_body(&mut self) -> ArenaBox<'a, FunctionBody<'a>> {
        if self.recognizing {
            return self.recognize_function_body();
        }
        let start = self.cur_start();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LCurly);

        // Add Return context, remove TopLevel context
        let strict = self.strict;
        let (directives, statements) = self.context(Context::Return, Context::TopLevel, |p| {
            p.parse_directives_and_statements(/* in_ts_namespace_body */ false)
        });
        self.strict = strict;

        self.expect_closing(Kind::RCurly, opening_span);
        FunctionBody::boxed(self.end_span(start), directives, statements, self)
    }

    /// Whether the body of the function starting at `start` parses lazily
    /// (`ParseOptions::lazy_function_bodies`).
    fn lazy_body_candidate(&self, start: u32, func_kind: FunctionKind) -> bool {
        (matches!(func_kind, FunctionKind::Declaration | FunctionKind::Expression)
            // A method's body only inside another lazily parsed body, which compiles from
            // its text again; never a constructor, whose `super()` its class decides.
            || self.lazy_depth > 0 && matches!(func_kind, FunctionKind::ClassMethod | FunctionKind::ObjectMethod))
            && self.lazy_start(start)
    }

    /// Whether the body of a class method starting at `start`, whose key is neither computed
    /// nor private, is skimmed (`ParseOptions::lazy_class_methods`): a plain method, getter or
    /// setter where a plain function's body would parse lazily, outside every lazy body.
    fn skims_class_method(&self, start: u32, func_kind: FunctionKind, async_or_generator: bool) -> bool {
        func_kind == FunctionKind::ClassMethod
            && !async_or_generator
            && self.options.lazy_class_methods
            && self.options.lazy_body_skim.is_some()
            && self.lazy_depth == 0
            && self.lazy_start(start)
    }

    /// The body of the class method starting at `start` skimmed and released, or `None` with
    /// the parser back at its opening brace. The method's own scope stays open in the
    /// early-error tracker, which the caller closes.
    fn skim_class_method_body(&mut self, start: u32, params: &ArenaBox<'a, FormalParameters<'a>>, head: Option<ParserCheckpoint<'a>>) -> Option<ArenaBox<'a, FunctionBody<'a>>> {
        let method = LazyFunction { start, id: None, params, r#async: false, generator: false, expression: false, form: LazyForm::Method, method: false };
        self.skim_lazy_function_body(&method, head)
    }

    /// Whether a function starting at `start` may parse lazily where it lies (outside every
    /// lazily parsed body, or inside one where nested bodies are) and, by the text before it,
    /// is not about to be called where it is made.
    pub(super) fn lazy_start(&self, start: u32) -> bool {
        self.options.lazy_function_bodies.is_some_and(|from| start >= from)
            // A body being recognized is released whole: the bodies inside it are only
            // recognized, and its compilation parses them again.
            && !self.recognizing
            && (self.lazy_depth == 0 || self.options.lazy_nested_bodies && self.options.lazy_body_check.is_some())
            && !self.source_text[..start as usize].trim_end().ends_with('(')
    }

    /// The block body of the arrow function starting at `start`, at its `{`: kept as its
    /// directives when the skim decides it (`ParseOptions::lazy_body_skim`), else parsed whole.
    pub(crate) fn parse_arrow_block_body(
        &mut self,
        start: u32,
        params: &ArenaBox<'a, FormalParameters<'a>>,
        r#async: bool,
        head: Option<ParserCheckpoint<'a>>,
    ) -> ArenaBox<'a, FunctionBody<'a>> {
        if !r#async && self.options.lazy_body_skim.is_some() && self.options.lazy_skim_arrows && self.lazy_start(start) {
            let arrow = LazyFunction { start, id: None, params, r#async, generator: false, expression: false, form: LazyForm::Arrow, method: false };
            if let Some(body) = self.skim_lazy_function_body(&arrow, head) {
                return body;
            }
        }
        self.context_add(Context::In, Self::parse_function_body)
    }

    /// A function body parsed, then released from the arena and kept as its directives,
    /// unless its parse left state that outlives it (`LazyBodyCheckpoint`).
    fn parse_lazy_function_body(&mut self, function: LazyFunction<'_, 'a>, head: Option<ParserCheckpoint<'a>>) -> ArenaBox<'a, FunctionBody<'a>> {
        if let Some(skip) = self.options.lazy_body_skip
            && !self.recognizing
            && self.lazy_depth == 0
            && let Some(body) = self.skip_lazy_body(skip, &function)
        {
            return body;
        }
        if let Some(body) = self.skim_lazy_function_body(&function, head) {
            return body;
        }
        if self.options.lazy_body_check.is_none() && self.options.lazy_body_skim.is_some() {
            // Without a check to settle it alone, a body the skim does not decide parses whole,
            // and the functions inside it may still be skimmed.
            let body = self.parse_function_body();
            self.early.end_body();
            return body;
        }
        // A function its consumer compiles at load needs its tree (`LazyBody::stub`).
        if self.options.lazy_recognize
            && self.options.lazy_body_check.is_some()
            && !function.r#async
            && !function.generator
            && !(function.expression && function.id.is_some())
            && let Some(body) = self.recognize_lazy_body(&function)
        {
            return body;
        }
        let recognizing = std::mem::replace(&mut self.recognizing, false);
        let body = self.parse_lazy_function_body_whole(&function);
        self.recognizing = recognizing;
        body
    }

    /// The body at the opening brace skipped unread to the end `skip` gives for it, and
    /// released (`ParseOptions::lazy_body_skip`), or `None` where it is read.
    fn skip_lazy_body(&mut self, skip: crate::LazyBodySkip, function: &LazyFunction<'_, 'a>) -> Option<ArenaBox<'a, FunctionBody<'a>>> {
        let start = self.cur_start();
        if !self.at(Kind::LCurly) {
            return None;
        }
        // SAFETY: `ParseOptions::lazy_body_skip` outlives the parse.
        let end = unsafe { skip.call(function.start) }?;
        let bytes = self.source_text.as_bytes();
        if end <= start + 1 || end as usize > bytes.len() || bytes[end as usize - 1] != b'}' {
            return None;
        }
        // A directive prologue (or a comment before one) is read.
        let first = bytes[start as usize + 1..end as usize - 1].iter().find(|byte| !byte.is_ascii_whitespace());
        if matches!(first, Some(b'"' | b'\'' | b'/')) {
            return None;
        }
        if !self.lexer.seek(end - 1) {
            return None;
        }
        let brace = self.token;
        self.token = self.lexer.next_token();
        if !self.at(Kind::RCurly) || self.cur_start() != end - 1 {
            // Not the body's end after all: it is read from its opening brace.
            if !self.lexer.seek(brace.end()) {
                self.set_fatal_error(diagnostics::expect_function_body(Span::new(start, end)));
            }
            self.token = brace;
            return None;
        }
        self.bump_any();
        // The skipped text was not lexed: a `#` in it counts as a private name it may hold.
        if memchr::memchr(b'#', &bytes[start as usize..end as usize]).is_some() {
            self.lexer.private_names += 1;
        }
        self.early.end_body();
        self.release_inside(Span::new(start, end), function.method);
        Some(FunctionBody::boxed(Span::new(start, end), ArenaVec::new_in(self), ArenaVec::new_in(self), self))
    }

    /// A lazily parsed body recognized (`js::recognize`), checked as a stand-in and released,
    /// or `None` with the parser back at its opening brace where it cannot be.
    fn recognize_lazy_body(&mut self, function: &LazyFunction<'_, 'a>) -> Option<ArenaBox<'a, FunctionBody<'a>>> {
        let allocator = self.allocator();
        let inherited_strict = self.strict;
        let marker = std::ptr::NonNull::from(allocator.alloc(0u8));
        let before = LazyBodyCheckpoint::of(self);
        let snapshot = self.early.snapshot_body()?;
        let rewind = self.recognizer_checkpoint();
        let verify = self.options.lazy_recognize_verify && !self.recognizing;
        let nested = self.lazy_depth > 0;
        self.lazy_depth += 1;
        let recognizing = std::mem::replace(&mut self.recognizing, true);
        let budget = self.rc_budget.is_none();
        if budget {
            self.rc_budget = Some(quench_stack::StackBudget::here());
        }
        let functions_before = self.rc_functions;
        let flat = budget && self.options.lazy_recognize_flat;
        if flat {
            self.rc_flat_from = Some(functions_before);
        }
        let private_names = self.lexer.private_names;
        let body = self.parse_function_body();
        let names_private = self.lexer.private_names != private_names;
        if flat {
            self.rc_flat_from = None;
        }
        // The body's own recognition counts once.
        let functions_inside = self.rc_functions - functions_before > 1;
        if budget {
            self.rc_budget = None;
        }
        self.recognizing = recognizing;
        self.lazy_depth -= 1;
        let span = body.span;
        let source = self.source_text.as_bytes().as_ptr_range();
        let in_source = |text: &str| text.is_empty() || source.contains(&text.as_ptr());
        let directives_in_source = body.directives.iter().all(|directive| {
            in_source(directive.directive.as_str())
                && in_source(directive.expression.value.as_str())
                && directive.expression.raw.is_none_or(|raw| in_source(raw.as_str()))
        });
        let escaped_after = self.lexer.escaped_strings.any_from(span.end)
            || self.lexer.escaped_templates.any_from(span.end);
        let after = LazyBodyCheckpoint::of(self);
        // A private member name checks only inside its class, which a body alone lacks.
        let private = nested && names_private;
        let fine = !private
            && self.fatal_error.is_none()
            && LazyBodyCheckpoint { arena_capacity: before.arena_capacity, ..after } == before
            && directives_in_source
            && !escaped_after
            && !self.early.body_unsure();
        if !fine {
            if verify {
                eprintln!("recognize-stat: whole {} {} p{} f{} c{} d{} e{} u{}", span.start, span.end - span.start, private, self.fatal_error.is_some(), LazyBodyCheckpoint { arena_capacity: before.arena_capacity, ..after } == before, directives_in_source, escaped_after, self.early.body_unsure());
            }
            self.recognizer_rewind(rewind);
            self.release_recognition(marker, &before);
            return None;
        }
        let verdict = self.early.end_body()?;
        if verify {
            // The parse that follows compares its verdict (`verify_recognized`).
            self.recognized_verdicts.push((span, verdict.free));
            self.early.restore_body(snapshot);
            self.recognizer_rewind(rewind);
            return None;
        }
        let check = self.options.lazy_body_check?;
        if !self.check_lazy_body(check, function, &body, inherited_strict, false, Some(&verdict), true, functions_inside) {
            self.early.restore_body(snapshot);
            self.recognizer_rewind(rewind);
            self.release_recognition(marker, &before);
            return None;
        }
        // The arena still has the chunk it had at `marker`: nothing allocated past the marker
        // is referenced (the parser state that can hold arena data is as it was before the
        // body, `fine` above), so it is released as `parse_lazy_function_body_whole` does.
        if allocator.capacity() == before.arena_capacity {
            // SAFETY: each directive's strings lie in the source text (`directives_in_source`).
            let directives: Vec<Directive<'a>> =
                body.directives.iter().map(|directive| unsafe { std::ptr::read(directive) }).collect();
            self.lexer.escaped_strings.remove_range(span.start..span.end);
            self.lexer.escaped_templates.remove_range(span.start..span.end);
            // SAFETY: as in `parse_lazy_function_body_whole`.
            unsafe { allocator.set_cursor_ptr(marker) };
            let directives = ArenaVec::from_iter_in(directives, &self.ast);
            self.release_inside(span, function.method);
            return Some(FunctionBody::boxed(span, directives, ArenaVec::new_in(self), self));
        }
        self.release_inside(span, function.method);
        Some(body)
    }

    /// Releases from the arena what a recognition rewound to the body's opening brace
    /// allocated, where the parser state is as it was before it (`LazyBodyCheckpoint`): the
    /// body is then parsed whole, which allocates its own (a body stopped at its first
    /// function inside, `ParseOptions::lazy_recognize_flat`, kept its partial reads before).
    fn release_recognition(&mut self, marker: std::ptr::NonNull<u8>, before: &LazyBodyCheckpoint) {
        if LazyBodyCheckpoint::of(self) != *before {
            return;
        }
        let start = self.cur_token().start();
        self.lexer.escaped_strings.truncate_before(start);
        self.lexer.escaped_templates.truncate_before(start);
        // SAFETY: the arena has the chunk it had at `marker` (its capacity is unchanged) and its
        // alignment is one byte. Nothing allocated after the marker is referenced: the parser is
        // rewound to the opening brace, its state that can hold arena data is as it was before
        // the recognition, and the escaped strings and templates read since are removed.
        unsafe { self.allocator().set_cursor_ptr(marker) };
    }

    /// Lists the body at `span` as released (a method's in `lazy_methods`), and drops the
    /// bodies released inside it, which end each list as they were released in source order.
    fn release_inside(&mut self, span: Span, method: bool) {
        let inside = |start: &u32| (span.start..span.end).contains(start);
        while self.lazy_methods.last().is_some_and(inside) {
            self.lazy_methods.pop();
        }
        while self.lazy_function_bodies.last().is_some_and(inside) {
            self.lazy_function_bodies.pop();
        }
        if method { self.lazy_methods.push(span.start) } else { self.lazy_function_bodies.push(span.start) }
    }

    /// Prints where the verdict of the body just parsed whole differs from the one recognized
    /// before it (`ParseOptions::lazy_recognize_verify`).
    fn verify_recognized(&mut self, span: Span, whole: Option<&crate::early::EarlyVerdict<'a>>, fine: bool) {
        let Some((_, free)) = self.recognized_verdicts.pop_if(|(recognized, _)| *recognized == span) else { return };
        match whole {
            Some(whole) if fine && !whole.unsure && whole.free == free => {}
            Some(whole) if fine && !whole.unsure => {
                eprintln!("recognize-verify: free names differ at {}: recognized {free:?} parsed {:?}", span.start, whole.free);
            }
            _ => {
                let end = (span.end as usize).min(span.start as usize + 400);
                let text = self.source_text.get(span.start as usize..end).unwrap_or_default();
                let head = self.source_text.get(..(span.start as usize).min(self.source_text.len())).unwrap_or_default();
                eprintln!("recognize-verify: recognized at {} but the parse is unsure ({:?}) or failed ({fine}, fatal {}): {text} AFTER {head}", span.start, whole.map(|whole| whole.unsure), self.fatal_error.is_some());
            }
        }
    }

    fn parse_lazy_function_body_whole(&mut self, function: &LazyFunction<'_, 'a>) -> ArenaBox<'a, FunctionBody<'a>> {
        let allocator = self.allocator();
        let inherited_strict = self.strict;
        // The arena allocates downward: everything the body allocates lies before this.
        let marker = std::ptr::NonNull::from(allocator.alloc(0u8));
        let checkpoint = LazyBodyCheckpoint::of(self);
        let call_assignment_targets = self.call_assignment_targets;
        let nested = self.lazy_depth > 0;
        self.lazy_depth += 1;
        let body = self.parse_function_body();
        self.lazy_depth -= 1;
        let verdict = self.early.end_body();
        let span = body.span;
        if !self.recognized_verdicts.is_empty() {
            let fine = self.fatal_error.is_none()
                && LazyBodyCheckpoint { arena_capacity: checkpoint.arena_capacity, ..LazyBodyCheckpoint::of(self) } == checkpoint;
            self.verify_recognized(span, verdict.as_ref(), fine);
        }
        // A private member name checks only inside its class, which a body alone lacks.
        if nested && self.source_text[span.start as usize..span.end as usize].contains('#') {
            return body;
        }
        let source = self.source_text.as_bytes().as_ptr_range();
        let in_source = |text: &str| text.is_empty() || source.contains(&text.as_ptr());
        let directives_in_source = body.directives.iter().all(|directive| {
            in_source(directive.directive.as_str())
                && in_source(directive.expression.value.as_str())
                && directive.expression.raw.is_none_or(|raw| in_source(raw.as_str()))
        });
        let escaped_after = self.lexer.escaped_strings.any_from(span.end)
            || self.lexer.escaped_templates.any_from(span.end);
        if checkpoint != LazyBodyCheckpoint::of(self)
            || self.fatal_error.is_some()
            || escaped_after
            || !directives_in_source
        {
            return body;
        }
        if let Some(check) = self.options.lazy_body_check {
            let targets = self.call_assignment_targets != call_assignment_targets;
            self.check_lazy_body(check, function, &body, inherited_strict, targets, verdict.as_ref(), false, true);
            // The check's nodes lie past the marker unless they took a new arena chunk; then
            // the body stays whole and the program's own checks see it as well.
            if allocator.capacity() != checkpoint.arena_capacity {
                return body;
            }
        }
        // SAFETY: each directive's strings lie in the source text, checked above, so a
        // bitwise copy holds nothing in the arena region released below.
        let directives: Vec<Directive<'a>> =
            body.directives.iter().map(|directive| unsafe { std::ptr::read(directive) }).collect();
        self.lexer.escaped_strings.remove_range(span.start..span.end);
        self.lexer.escaped_templates.remove_range(span.start..span.end);
        // SAFETY: the arena has the same chunk as at `marker` (its capacity is unchanged), the
        // marker lies in it and the arena's alignment is one byte. Nothing allocated after the
        // marker is referenced any more: the body is dropped, the parser state that can hold
        // arena data is as it was at the checkpoint, and the escaped strings and templates
        // inside the body are removed.
        unsafe { allocator.set_cursor_ptr(marker) };
        // The bodies released inside it leave the tree with it: they were released after its
        // body began, so they end each list.
        self.release_inside(span, function.method);
        let directives = ArenaVec::from_iter_in(directives, &self.ast);
        FunctionBody::boxed(span, directives, ArenaVec::new_in(self), self)
    }

    /// The body kept as its directives after one scan of its tokens (`ParseOptions::lazy_body_skim`),
    /// when the scan finds it valid, leaves no state behind and the consumer takes the function.
    /// Otherwise the parser is as it was and the body parses whole.
    fn skim_lazy_function_body(&mut self, function: &LazyFunction<'_, 'a>, head: Option<ParserCheckpoint<'a>>) -> Option<ArenaBox<'a, FunctionBody<'a>>> {
        let skim = self.options.lazy_body_skim?;
        // A body inside a lazily parsed one is checked, not skimmed: its skim would leave the
        // enclosing body's early verdict unsure, which its own check then cannot take.
        if self.lazy_depth > 0 || function.r#async || function.generator || function.method || function.expression && function.id.is_some() || !self.at(Kind::LCurly) {
            return None;
        }
        let allocator = self.allocator();
        let marker = std::ptr::NonNull::from(allocator.alloc(0u8));
        let mut before = LazyBodyCheckpoint::of(self);
        let capacity = before.arena_capacity;
        let checkpoint = self.checkpoint();
        let inherited_strict = self.strict;
        let start = self.cur_start();
        let skimmed = self.skim_function_body(function.params, function.form, head);
        before.arena_capacity = allocator.capacity();
        let taken = skimmed.filter(|_| before == LazyBodyCheckpoint::of(self)).filter(|skimmed| {
            let end = self.prev_token_end;
            let body = crate::SkimmedBody {
                start: function.start,
                end,
                id: function.id.map(|id| id.name.as_str()),
                free_names: &skimmed.free_names,
                // A method is a function whatever lies around its class.
                in_function: self.ctx.has_return() || function.form == LazyForm::Method,
                arrow: function.form == LazyForm::Arrow,
                names_super: skimmed.names_super,
            };
            // SAFETY: `ParseOptions::lazy_body_skim` outlives the parse.
            unsafe { skim.call(body) }
        });
        self.strict = inherited_strict;
        let Some(Skimmed { directives: skimmed_directives, .. }) = taken else {
            self.rewind(checkpoint);
            return None;
        };
        let span = Span::new(start, self.prev_token_end);
        self.release_skimmed_strings(span, marker, capacity);
        // The early-error tracker saw none of the body's tokens: a plain function's tracked body
        // ends here, and the body around it, missing what it reads, is unsure.
        if function.form == LazyForm::Plain {
            self.early.end_body();
        }
        self.early.mark_unsure();
        self.release_inside(span, false);
        let mut directives = ArenaVec::new_in(self);
        for (start, literal_end, end) in skimmed_directives {
            let text = &self.source_text[start as usize + 1..literal_end as usize - 1];
            let literal = StringLiteral::new(Span::new(start, literal_end), text, None, self);
            directives.push(Directive::new(Span::new(start, end), literal, text, self));
        }
        Some(FunctionBody::boxed(span, directives, ArenaVec::new_in(self), self))
    }

    /// The arena the skim used is released, with the escaped strings and templates it decoded
    /// inside `span`, unless one past it shares the arena region.
    fn release_skimmed_strings(&mut self, span: Span, marker: std::ptr::NonNull<u8>, capacity: usize) {
        let escaped_after = self.lexer.escaped_strings.any_from(span.end)
            || self.lexer.escaped_templates.any_from(span.end);
        if escaped_after {
            return;
        }
        self.lexer.escaped_strings.remove_range(span.start..span.end);
        self.lexer.escaped_templates.remove_range(span.start..span.end);
        if self.allocator().capacity() == capacity {
            // SAFETY: as in `parse_lazy_function_body`: the arena has the chunk of `marker`, and
            // nothing allocated after it is referenced any more.
            unsafe { self.allocator().set_cursor_ptr(marker) };
        }
    }

    /// Calls `check` with `function` and its `body` alone as a program, as its source
    /// would parse alone in the strictness it inherits, before the body is released.
    fn check_lazy_body(
        &self,
        check: crate::LazyBodyCheck,
        function: &LazyFunction<'_, 'a>,
        body: &ArenaBox<'a, FunctionBody<'a>>,
        inherited_strict: bool,
        call_assignment_targets: bool,
        verdict: Option<&crate::early::EarlyVerdict<'a>>,
        stub: bool,
        functions_inside: bool,
    ) -> bool {
        let end = body.span.end;
        let span = Span::new(function.start, end);
        // SAFETY: bitwise copies of the arena boxes and the binding, read only through the
        // shared program below while the originals are not touched; the copies own nothing
        // that drops.
        let (id, params, body) = unsafe {
            (function.id.map(|id| std::ptr::read(id)), std::ptr::read(function.params), std::ptr::read(body))
        };
        let r#type = if function.expression || function.method { FunctionType::FunctionExpression } else { FunctionType::FunctionDeclaration };
        let node = Function::boxed(span, r#type, id, function.generator, function.r#async, false, None, None, params, None, Some(body), self);
        let statement = if function.method {
            // `({ m(...) { ... } })`: a method's syntax (`super.x`, unique parameters) without
            // its class or object, whose own checks the program around it made.
            let key = PropertyKey::StaticIdentifier(IdentifierName::boxed(Span::empty(function.start), "m", self));
            let property = ObjectProperty::boxed(span, PropertyKind::Init, key, Expression::FunctionExpression(node), true, false, false, self);
            let mut properties = ArenaVec::new_in(self);
            properties.push(ObjectPropertyKind::ObjectProperty(property));
            let object = ObjectExpression::boxed(span, properties, self);
            Statement::ExpressionStatement(ExpressionStatement::boxed(span, Expression::ObjectExpression(object), self))
        } else if function.expression {
            Statement::ExpressionStatement(ExpressionStatement::boxed(span, Expression::FunctionExpression(node), self))
        } else {
            Statement::FunctionDeclaration(node)
        };
        let mut directives = ArenaVec::new_in(self);
        if inherited_strict && !self.source_type.is_module() {
            let empty = Span::empty(function.start);
            directives.push(Directive::new(empty, StringLiteral::new(empty, "use strict", None, self), "use strict", self));
        }
        let mut statements = ArenaVec::new_in(self);
        statements.push(statement);
        let program = Program::new(span, self.source_type, self.source_text, [], None, directives, statements, self);
        let in_function = self.ctx.has_return();
        // SAFETY: `ParseOptions::lazy_body_check` outlives the parse.
        let early = verdict.filter(|verdict| !verdict.unsure).map(|verdict| verdict.free.as_slice());
        let accepted = unsafe { check.call(crate::LazyBody { program: &program, call_assignment_targets, in_function, early, stub, functions_inside }) };
        std::mem::forget(program);
        accepted
    }

    pub(crate) fn parse_formal_parameters(
        &mut self,
        func_kind: FunctionKind,
        params_kind: FormalParameterKind,
    ) -> (
        Option<ArenaBox<'a, TSThisParameter<'a>>>,
        ArenaBox<'a, FormalParameters<'a>>,
    ) {
        let start = self.cur_start();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LParen);
        let this_param = if self.is_ts && self.at(Kind::This) {
            let param = self.parse_ts_this_parameter();
            if !self.at(Kind::RParen) {
                self.expect(Kind::Comma);
            }
            Some(param)
        } else {
            None
        };
        self.early.set_in_params(true);
        let binding_kind = self.binding_kind.replace(crate::early::DeclKind::Param);
        let (list, rest) = self.parse_formal_parameters_list(func_kind, opening_span);
        self.binding_kind = binding_kind;
        self.early.set_in_params(false);
        self.expect(Kind::RParen);

        let formal_parameters =
            FormalParameters::boxed(self.end_span(start), params_kind, list, rest, self);
        (this_param, formal_parameters)
    }

    fn parse_formal_parameters_list(
        &mut self,
        func_kind: FunctionKind,
        opening_span: Span,
    ) -> (
        ArenaVec<'a, FormalParameter<'a>>,
        Option<ArenaBox<'a, FormalParameterRest<'a>>>,
    ) {
        let mut list = ArenaVec::new_in(self);
        let rest = self.parse_formal_parameters_list_into(&mut list, func_kind, opening_span);
        (list, rest)
    }

    #[expect(clippy::inline_always)]
    #[inline(always)]
    fn parse_formal_parameters_list_into(
        &mut self,
        list: &mut ArenaVec<'a, FormalParameter<'a>>,
        func_kind: FunctionKind,
        opening_span: Span,
    ) -> Option<ArenaBox<'a, FormalParameterRest<'a>>> {
        let mut rest: Option<ArenaBox<'a, FormalParameterRest<'a>>> = None;
        let mut first = true;
        let mut has_optional = false;

        loop {
            let kind = self.cur_kind();
            if kind == Kind::RParen
                || matches!(kind, Kind::Eof | Kind::Undetermined)
                || self.fatal_error.is_some()
            {
                break;
            }

            if first {
                first = false;
            } else {
                let comma_span = self.cur_token().span();
                if kind != Kind::Comma {
                    let error = diagnostics::expect_closing_or_separator(
                        Kind::RParen.to_str(),
                        Kind::Comma.to_str(),
                        kind.to_str(),
                        comma_span,
                        opening_span,
                    );
                    self.set_fatal_error(error);
                    break;
                }
                self.bump_any();
                let kind = self.cur_kind();
                if kind == Kind::RParen {
                    if rest.is_some() && !self.ctx.has_ambient() {
                        self.error(diagnostics::rest_element_trailing_comma(comma_span));
                    }
                    break;
                }
            }

            if let Some(r) = &rest {
                self.set_fatal_error(diagnostics::rest_parameter_last(
                    r.type_annotation.as_ref().map_or_else(
                        || r.rest.span,
                        |type_annotation| r.rest.span.merge(type_annotation.span()),
                    ),
                ));
                break;
            }

            let start = self.cur_start();
            let decorators = self.parse_decorators();

            if self.at(Kind::Dot3) {
                let rest_element = self.parse_rest_element_for_formal_parameter();
                let type_annotation = if self.is_ts {
                    self.parse_ts_type_annotation()
                } else {
                    None
                };

                let are_decorators_allowed = matches!(
                    func_kind,
                    FunctionKind::ClassMethod | FunctionKind::Constructor
                ) && self.is_ts;
                if !are_decorators_allowed {
                    for decorator in &decorators {
                        self.error(diagnostics::decorators_are_not_valid_here(decorator.span));
                    }
                }

                rest = Some(FormalParameterRest::boxed(
                    self.end_span(start),
                    decorators,
                    rest_element,
                    type_annotation,
                    self,
                ));
            } else {
                let param =
                    self.parse_formal_parameter_with_decorators(func_kind, start, decorators);
                if param.optional {
                    has_optional = true;
                } else if has_optional && param.initializer.is_none() {
                    self.error(diagnostics::required_parameter_after_optional_parameter(
                        param.span,
                    ));
                }
                list.push(param);
            }
        }

        rest
    }

    fn parse_formal_parameter_with_decorators(
        &mut self,
        func_kind: FunctionKind,
        start: u32,
        decorators: ArenaVec<'a, Decorator<'a>>,
    ) -> FormalParameter<'a> {
        let modifiers = self.parse_modifiers(false, false);
        if self.is_ts {
            let allowed_modifiers = if func_kind == FunctionKind::Constructor {
                ModifierKinds::new([
                    ModifierKind::Public,
                    ModifierKind::Private,
                    ModifierKind::Protected,
                    ModifierKind::Override,
                    ModifierKind::Readonly,
                ])
            } else {
                ModifierKinds::none()
            };
            self.verify_modifiers(
                &modifiers,
                allowed_modifiers,
                true,
                diagnostics::cannot_appear_on_a_parameter,
            );
        } else {
            self.verify_modifiers(
                &modifiers,
                ModifierKinds::none(),
                true,
                diagnostics::parameter_modifiers_in_ts,
            );
        }
        let pattern = self.parse_binding_pattern();

        let optional = self.is_ts && self.eat(Kind::Question);
        let type_annotation = self.parse_ts_type_annotation();

        // Now parse the initializer if present
        let init = if self.eat(Kind::Eq) {
            let init = self.context_add(
                Context::In,
                ParserImpl::parse_assignment_expression_or_higher,
            );
            if optional {
                self.error(
                    diagnostics::a_parameter_cannot_have_question_mark_and_initializer(
                        pattern.span(),
                    ),
                );
            }
            Some(ArenaBox::new_in(init, self))
        } else {
            None
        };

        let is_parameter_property = modifiers.contains_accessibility()
            || modifiers.contains_readonly()
            || modifiers.contains_override();
        if is_parameter_property {
            if let Some(ident) = pattern.get_binding_identifier() {
                if func_kind == FunctionKind::Constructor && ident.name == "constructor" {
                    self.error(diagnostics::constructor_cannot_be_parameter_property_name(
                        ident.span,
                    ));
                }
            } else {
                self.error(diagnostics::parameter_property_cannot_be_binding_pattern(
                    Span::new(start, self.prev_token_end),
                ));
            }
        }

        let are_decorators_allowed = matches!(
            func_kind,
            FunctionKind::ClassMethod | FunctionKind::Constructor
        ) && self.is_ts;
        if !are_decorators_allowed {
            for decorator in &decorators {
                self.error(diagnostics::decorators_are_not_valid_here(decorator.span));
            }
        }
        FormalParameter::new(
            self.end_span(start),
            decorators,
            pattern,
            type_annotation,
            init,
            optional,
            modifiers.accessibility(),
            modifiers.contains_readonly(),
            modifiers.contains_override(),
            self,
        )
    }

    pub(crate) fn parse_function(
        &mut self,
        start: u32,
        id: Option<BindingIdentifier<'a>>,
        r#async: bool,
        generator: Option<u32>,
        func_kind: FunctionKind,
        param_kind: FormalParameterKind,
        modifiers: &Modifiers,
    ) -> ArenaBox<'a, Function<'a>> {
        let ctx = self.ctx;
        let lazy = self.lazy_body_candidate(start, func_kind);
        let skim_method = std::mem::take(&mut self.skim_method)
            && self.skims_class_method(start, func_kind, r#async || generator.is_some());
        if let Some(id) = &id
            && matches!(func_kind, FunctionKind::Declaration | FunctionKind::DefaultExport)
        {
            self.declare_function_name(id.name, r#async || generator.is_some());
        }
        let home = match func_kind {
            FunctionKind::ClassMethod | FunctionKind::ObjectMethod => crate::early::Home::Method,
            FunctionKind::Constructor => self.early.method_home(true),
            _ => crate::early::Home::Plain,
        };
        if lazy {
            self.early.begin_body();
        } else {
            self.early.enter_function(Some(home));
        }
        if let Some(id) = &id
            && func_kind == FunctionKind::Expression
        {
            self.early.declare(id.name, crate::early::DeclKind::OwnName);
        }
        // `new.target` is allowed in a function's parameters and body (but not arrow
        // functions, which are parsed via `parse_function_body` directly).
        self.ctx = self
            .ctx
            .and_in(true)
            .and_await(r#async)
            .and_yield(generator.is_some())
            .and_new_target(true);
        let type_parameters = self.parse_ts_type_parameters();
        let head = (lazy || skim_method).then(|| self.checkpoint());
        let (this_param, params) = self.parse_formal_parameters(func_kind, param_kind);
        let return_type = if self.is_ts {
            self.parse_ts_return_type_annotation()
        } else {
            None
        };
        self.early.set_simple_params(crate::early::simple_parameters(&params));
        let body = if self.at(Kind::LCurly) || func_kind == FunctionKind::Expression {
            Some(if lazy {
                self.parse_lazy_function_body(LazyFunction {
                    start,
                    id: id.as_ref(),
                    params: &params,
                    r#async,
                    generator: generator.is_some(),
                    expression: func_kind == FunctionKind::Expression,
                    form: LazyForm::Plain,
                    method: matches!(func_kind, FunctionKind::ClassMethod | FunctionKind::ObjectMethod),
                }, head)
            } else {
                let skimmed = skim_method.then(|| self.skim_class_method_body(start, &params, head)).flatten();
                let body = skimmed.unwrap_or_else(|| self.parse_function_body());
                self.early.exit_function();
                body
            })
        } else {
            if lazy {
                self.early.end_body();
            } else {
                self.early.exit_function();
            }
            None
        };
        self.ctx = self
            .ctx
            .and_in(ctx.has_in())
            .and_await(ctx.has_await())
            .and_yield(ctx.has_yield())
            .and_new_target(ctx.has_new_target());
        if (!self.is_ts || matches!(func_kind, FunctionKind::ObjectMethod)) && body.is_none() {
            return self.fatal_error(diagnostics::expect_function_body(self.end_span(start)));
        }
        let function_type = match func_kind {
            FunctionKind::Declaration | FunctionKind::DefaultExport => {
                if body.is_none() {
                    FunctionType::TSDeclareFunction
                } else {
                    FunctionType::FunctionDeclaration
                }
            }
            FunctionKind::Expression
            | FunctionKind::ClassMethod
            | FunctionKind::Constructor
            | FunctionKind::ObjectMethod => {
                if body.is_none() {
                    FunctionType::TSEmptyBodyFunctionExpression
                } else {
                    FunctionType::FunctionExpression
                }
            }
            FunctionKind::TSDeclaration => FunctionType::TSDeclareFunction,
        };

        if FunctionType::TSDeclareFunction == function_type
            || FunctionType::TSEmptyBodyFunctionExpression == function_type
        {
            self.asi();
        }

        // A function declaration's implementation (body) cannot be declared in an ambient context,
        // whether the ambient context comes from the function's own `declare` modifier or is
        // inherited from an enclosing `declare module`/`declare namespace` or a `.d.ts` file
        // (TS1183). Class methods are checked separately in `check_method_definition`, so they are
        // excluded here to avoid a duplicate diagnostic.
        if ctx.has_ambient()
            && matches!(
                func_kind,
                FunctionKind::Declaration
                    | FunctionKind::DefaultExport
                    | FunctionKind::TSDeclaration
            )
            && let Some(body) = &body
        {
            self.error(diagnostics::implementation_in_ambient(Span::empty(
                body.span.start,
            )));
        }

        if let Some(generator) = generator {
            if ctx.has_ambient() {
                self.error(diagnostics::generator_in_ambient_context(
                    self.end_span(generator),
                ));
            } else if body.is_none() {
                self.error(diagnostics::overload_signature_generator(
                    self.end_span(start.min(generator)),
                ));
            }
        }
        self.verify_modifiers(
            modifiers,
            ModifierKinds::new([ModifierKind::Declare, ModifierKind::Async]),
            true,
            diagnostics::modifier_cannot_be_used_here,
        );

        Function::boxed(
            self.end_span(start),
            function_type,
            id,
            generator.is_some(),
            r#async,
            modifiers.contains_declare(),
            type_parameters,
            this_param,
            params,
            return_type,
            body,
            self,
        )
    }

    /// [Function Declaration](https://tc39.es/ecma262/#prod-FunctionDeclaration)
    pub(crate) fn parse_function_declaration(
        &mut self,
        start: u32,
        r#async: bool,
        stmt_ctx: StatementContext,
    ) -> Statement<'a> {
        let func_kind = FunctionKind::Declaration;
        let decl = self.parse_function_impl(start, r#async, func_kind);
        if stmt_ctx.is_single_statement() {
            self.early.mark_unsure();
            if decl.r#async {
                self.error(diagnostics::async_function_declaration(Span::new(
                    decl.span.start,
                    decl.params.span.end,
                )));
            } else if decl.generator {
                self.error(diagnostics::generator_function_declaration(Span::new(
                    decl.span.start,
                    decl.params.span.end,
                )));
            }
        }
        Statement::FunctionDeclaration(decl)
    }

    /// Parse function implementation in Javascript, cursor
    /// at `function` or `async function`
    pub(crate) fn parse_function_impl(
        &mut self,
        start: u32,
        r#async: bool,
        func_kind: FunctionKind,
    ) -> ArenaBox<'a, Function<'a>> {
        self.expect(Kind::Function);
        let generator = self.eat(Kind::Star).then_some(self.prev_token_end - 1);
        let id = self.parse_function_id(func_kind, r#async, generator.is_some());
        self.parse_function(
            start,
            id,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::FormalParameter,
            &Modifiers::empty(),
        )
    }

    /// Parse function implementation in Typescript, cursor
    /// at `function`
    pub(crate) fn parse_ts_function_impl(
        &mut self,
        start: u32,
        func_kind: FunctionKind,
        modifiers: &Modifiers,
    ) -> ArenaBox<'a, Function<'a>> {
        let r#async = modifiers.contains(ModifierKind::Async);
        self.expect(Kind::Function);
        let generator = self.eat(Kind::Star).then_some(self.prev_token_end - 1);
        let id = self.parse_function_id(func_kind, r#async, generator.is_some());
        self.parse_function(
            start,
            id,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::FormalParameter,
            modifiers,
        )
    }

    /// [Function Expression](https://tc39.es/ecma262/#prod-FunctionExpression)
    pub(crate) fn parse_function_expression(
        &mut self,
        start: u32,
        r#async: bool,
    ) -> Expression<'a> {
        let func_kind = FunctionKind::Expression;
        self.expect(Kind::Function);

        let generator = self.eat(Kind::Star).then_some(self.prev_token_end - 1);
        let id = self.parse_function_id(func_kind, r#async, generator.is_some());
        let function = self.parse_function(
            start,
            id,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::FormalParameter,
            &Modifiers::empty(),
        );
        Expression::FunctionExpression(function)
    }

    /// Section 15.4 Method Definitions
    /// `ClassElementName` ( `UniqueFormalParameters` ) { `FunctionBody` }
    /// * `GeneratorMethod`
    ///   * `ClassElementName`
    /// * `AsyncMethod`
    ///   async `ClassElementName`
    /// * `AsyncGeneratorMethod`
    ///   async * `ClassElementName`
    pub(crate) fn parse_method(
        &mut self,
        r#async: bool,
        generator: Option<u32>,
        func_kind: FunctionKind,
    ) -> ArenaBox<'a, Function<'a>> {
        let start = self.cur_start();
        self.parse_function(
            start,
            None,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::UniqueFormalParameters,
            &Modifiers::empty(),
        )
    }

    /// Section 15.5 Yield Expression
    /// yield
    /// yield [no `LineTerminator` here] `AssignmentExpression`
    /// yield [no `LineTerminator` here] * `AssignmentExpression`
    /// Declares a function declaration's name where it lies (`early`): lexical in a block,
    /// where a plain function in sloppy code also binds in the function around it (Annex B),
    /// which the tracker does not model.
    pub(super) fn declare_function_name(&mut self, name: oxc_str::Ident<'a>, async_or_generator: bool) {
        if !self.early.active() {
            return;
        }
        if self.early.in_block() {
            if !self.strict && !async_or_generator {
                self.early.mark_unsure();
            }
            self.early.declare(name, crate::early::DeclKind::Lexical);
        } else {
            self.early.declare(name, crate::early::DeclKind::Var);
        }
    }

    pub(crate) fn parse_yield_expression(&mut self) -> Expression<'a> {
        self.early.await_or_yield(false);
        let start = self.cur_start();
        self.bump_any(); // advance `yield`

        let has_yield = self.ctx.has_yield();
        if !has_yield {
            self.error(diagnostics::yield_expression(Span::sized(start, 5)));
        }

        let mut delegate = false;
        let mut argument = None;

        if !self.cur_token().is_on_new_line() {
            delegate = self.eat(Kind::Star);
            let not_assignment_expr = matches!(
                self.cur_kind(),
                Kind::Semicolon
                    | Kind::Eof
                    | Kind::RCurly
                    | Kind::RParen
                    | Kind::RBrack
                    | Kind::Colon
                    | Kind::Comma
            );
            if !not_assignment_expr || delegate {
                self.ctx = self.ctx.union_yield_if(true);
                argument = Some(self.parse_assignment_expression_or_higher());
                self.ctx = self.ctx.and_yield(has_yield);
            }
        }

        Expression::new_yield_expression(self.end_span(start), delegate, argument, self)
    }

    // id: None - for AnonymousDefaultExportedFunctionDeclaration
    pub(crate) fn parse_function_id(
        &mut self,
        func_kind: FunctionKind,
        r#async: bool,
        generator: bool,
    ) -> Option<BindingIdentifier<'a>> {
        let kind = self.cur_kind();
        if kind.is_binding_identifier() {
            let mut ctx = self.ctx;
            if func_kind.is_expression() {
                ctx = ctx.and_await(r#async).and_yield(generator);
            }
            self.check_identifier(kind, ctx);

            let (span, name) = self.parse_identifier_kind(Kind::Ident);
            Some(BindingIdentifier::new(span, name, self))
        } else {
            if func_kind.is_id_required() {
                match self.cur_kind() {
                    Kind::LParen => {
                        self.error(diagnostics::expect_function_name(self.cur_token().span()));
                    }
                    kind if kind.is_reserved_keyword() => self.expect_without_advance(Kind::Ident),
                    _ => {}
                }
            }

            None
        }
    }
}

/// The parts of a function whose body parses lazily that a check of it alone reads.
struct LazyFunction<'f, 'a> {
    start: u32,
    id: Option<&'f BindingIdentifier<'a>>,
    params: &'f ArenaBox<'a, FormalParameters<'a>>,
    r#async: bool,
    generator: bool,
    expression: bool,
    /// What kind of function it is; the body of an arrow only the skim takes.
    form: LazyForm,
    /// A class or object method's: it checks alone as an object literal's method.
    method: bool,
}

/// The parser state a function body's parse can extend with data that outlives the body.
/// A lazily parsed body releases the arena it used only when this is unchanged.
#[derive(PartialEq, Eq)]
pub(crate) struct LazyBodyCheckpoint {
    /// Arena chunks only grow: the same capacity means the same current chunk.
    arena_capacity: usize,
    errors: usize,
    deferred_script_errors: usize,
    lexer_errors: usize,
    deferred_module_errors: usize,
    tokens: usize,
    module_record_entries: usize,
    cover_initialized_names: usize,
    potential_await_reparses: usize,
}

impl LazyBodyCheckpoint {
    pub(crate) fn of<C: Config>(parser: &ParserImpl<'_, C>) -> Self {
        Self {
            arena_capacity: parser.allocator().capacity(),
            errors: parser.errors.len(),
            deferred_script_errors: parser.deferred_script_errors.len(),
            lexer_errors: parser.lexer.errors.len(),
            deferred_module_errors: parser.lexer.deferred_module_errors.len(),
            tokens: parser.lexer.tokens_len(),
            module_record_entries: parser.module_record_builder.function_body_entries(),
            cover_initialized_names: parser.state.cover_initialized_name.len(),
            potential_await_reparses: parser.state.potential_await_reparse.len(),
        }
    }
}
