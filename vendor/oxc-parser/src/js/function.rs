use oxc_allocator::{ArenaBox, ArenaVec, GetAllocator};
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::FunctionKind;
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
        let start = self.cur_start();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LCurly);

        // Add Return context, remove TopLevel context
        let (directives, statements) = self.context(Context::Return, Context::TopLevel, |p| {
            p.parse_directives_and_statements(/* in_ts_namespace_body */ false)
        });

        self.expect_closing(Kind::RCurly, opening_span);
        FunctionBody::boxed(self.end_span(start), directives, statements, self)
    }

    /// Whether the body of the function starting at `start` parses lazily
    /// (`ParseOptions::lazy_function_bodies`).
    fn lazy_body_candidate(&self, start: u32, func_kind: FunctionKind) -> bool {
        self.options.lazy_function_bodies.is_some_and(|from| start >= from)
            && !self.is_ts
            && self.lazy_depth == 0
            && matches!(func_kind, FunctionKind::Declaration | FunctionKind::Expression)
            && !self.source_text[..start as usize].trim_end().ends_with('(')
    }

    /// A function body parsed, then released from the arena and kept as its directives,
    /// unless its parse left state that outlives it (`LazyBodyCheckpoint`).
    fn parse_lazy_function_body(&mut self) -> ArenaBox<'a, FunctionBody<'a>> {
        let allocator = self.allocator();
        // The arena allocates downward: everything the body allocates lies before this.
        let marker = std::ptr::NonNull::from(allocator.alloc(0u8));
        let checkpoint = LazyBodyCheckpoint::of(self);
        self.lazy_depth += 1;
        let body = self.parse_function_body();
        self.lazy_depth -= 1;
        let span = body.span;
        let source = self.source_text.as_bytes().as_ptr_range();
        let in_source = |text: &str| text.is_empty() || source.contains(&text.as_ptr());
        let directives_in_source = body.directives.iter().all(|directive| {
            in_source(directive.directive.as_str())
                && in_source(directive.expression.value.as_str())
                && directive.expression.raw.is_none_or(|raw| in_source(raw.as_str()))
        });
        let escaped_after = self.lexer.escaped_strings.keys().any(|start| *start >= span.end)
            || self.lexer.escaped_templates.keys().any(|start| *start >= span.end);
        if checkpoint != LazyBodyCheckpoint::of(self)
            || self.fatal_error.is_some()
            || escaped_after
            || !directives_in_source
        {
            return body;
        }
        // SAFETY: each directive's strings lie in the source text, checked above, so a
        // bitwise copy holds nothing in the arena region released below.
        let directives: Vec<Directive<'a>> =
            body.directives.iter().map(|directive| unsafe { std::ptr::read(directive) }).collect();
        let inside = |start: &u32| (span.start..span.end).contains(start);
        self.lexer.escaped_strings.retain(|start, _| !inside(start));
        self.lexer.escaped_templates.retain(|start, _| !inside(start));
        // SAFETY: the arena has the same chunk as at `marker` (its capacity is unchanged), the
        // marker lies in it and the arena's alignment is one byte. Nothing allocated after the
        // marker is referenced any more: the body is dropped, the parser state that can hold
        // arena data is as it was at the checkpoint, and the escaped strings and templates
        // inside the body are removed.
        unsafe { allocator.set_cursor_ptr(marker) };
        self.lazy_function_bodies.push(span.start);
        let directives = ArenaVec::from_iter_in(directives, &self.ast);
        FunctionBody::boxed(span, directives, ArenaVec::new_in(self), self)
    }

    pub(crate) fn parse_formal_parameters(
        &mut self,
        func_kind: FunctionKind,
        params_kind: FormalParameterKind,
    ) -> (Option<ArenaBox<'a, TSThisParameter<'a>>>, ArenaBox<'a, FormalParameters<'a>>) {
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
        let (list, rest) = self.parse_formal_parameters_list(func_kind, opening_span);
        self.expect(Kind::RParen);

        let formal_parameters =
            FormalParameters::boxed(self.end_span(start), params_kind, list, rest, self);
        (this_param, formal_parameters)
    }

    fn parse_formal_parameters_list(
        &mut self,
        func_kind: FunctionKind,
        opening_span: Span,
    ) -> (ArenaVec<'a, FormalParameter<'a>>, Option<ArenaBox<'a, FormalParameterRest<'a>>>) {
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
                let type_annotation =
                    if self.is_ts { self.parse_ts_type_annotation() } else { None };

                let are_decorators_allowed =
                    matches!(func_kind, FunctionKind::ClassMethod | FunctionKind::Constructor)
                        && self.is_ts;
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
            let init =
                self.context_add(Context::In, ParserImpl::parse_assignment_expression_or_higher);
            if optional {
                self.error(diagnostics::a_parameter_cannot_have_question_mark_and_initializer(
                    pattern.span(),
                ));
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
                self.error(diagnostics::parameter_property_cannot_be_binding_pattern(Span::new(
                    start,
                    self.prev_token_end,
                )));
            }
        }

        let are_decorators_allowed =
            matches!(func_kind, FunctionKind::ClassMethod | FunctionKind::Constructor)
                && self.is_ts;
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
        // `new.target` is allowed in a function's parameters and body (but not arrow
        // functions, which are parsed via `parse_function_body` directly).
        self.ctx = self
            .ctx
            .and_in(true)
            .and_await(r#async)
            .and_yield(generator.is_some())
            .and_new_target(true);
        let type_parameters = self.parse_ts_type_parameters();
        let (this_param, params) = self.parse_formal_parameters(func_kind, param_kind);
        let return_type = if self.is_ts { self.parse_ts_return_type_annotation() } else { None };
        let body = if self.at(Kind::LCurly) || func_kind == FunctionKind::Expression {
            Some(if self.lazy_body_candidate(start, func_kind) {
                self.parse_lazy_function_body()
            } else {
                self.parse_function_body()
            })
        } else {
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
            self.error(diagnostics::implementation_in_ambient(Span::empty(body.span.start)));
        }

        if let Some(generator) = generator {
            if ctx.has_ambient() {
                self.error(diagnostics::generator_in_ambient_context(self.end_span(generator)));
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
    pub(crate) fn parse_yield_expression(&mut self) -> Expression<'a> {
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

/// The parser state a function body's parse can extend with data that outlives the body.
/// A lazily parsed body releases the arena it used only when this is unchanged.
#[derive(PartialEq, Eq)]
struct LazyBodyCheckpoint {
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
    fn of<C: Config>(parser: &ParserImpl<'_, C>) -> Self {
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
