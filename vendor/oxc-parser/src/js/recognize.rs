//! A recognizer for the function bodies inside a lazily parsed body (Quench): it reads their
//! statements and expressions without building them, feeding the early-error tracker
//! (`crate::early`) as the parse does, so a released body costs its tokens and little more.
//!
//! It stands in for `parse_function_body` while `ParserImpl::recognizing` is set, which
//! `parse_lazy_function_body` does for a body it releases. Constructs it does not read itself
//! (function and class expressions, templates, regular expressions, numbers, arrow functions,
//! destructuring, rare statements) it hands to the parser, whose nodes then lie unreferenced in
//! the arena the release frees, and whose own function bodies come back here. Each statement
//! it reads starts from a checkpoint: anything it does not accept, an error included, rewinds
//! that statement and hands it to the parser, so the parser alone decides what is an error and
//! reports it. The body around then is not released as recognized (`recognize_lazy_body`).

use oxc_allocator::{ArenaBox, ArenaVec};
use oxc_ast::ast::*;
use oxc_str::Str;

use super::expression::legacy_escape;
use crate::{
    Context, ParserConfig as Config, ParserImpl, StatementContext,
    cursor::ParserCheckpoint,
    early::{DeclKind, ScopeKind},
    lexer::Kind,
};

/// A construct the recognizer does not accept: its statement goes to the parser.
type Rc<T> = Result<T, ()>;

/// What an expression is, as far as the rules on what may follow it need.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// An identifier reference alone: the last one read (`ParserImpl::rc_ident`).
    Ident,
    /// A member expression, outside an optional chain.
    Member,
    /// A parenthesized identifier reference (`ParserImpl::rc_ident`), or member expression.
    ParenIdent,
    ParenMember,
    /// An optional chain.
    Chain,
    /// An object or array literal alone (an assignment pattern when `=` follows).
    Literal,
    /// `super` alone.
    Super,
    Other,
}

/// The parser state a statement the recognizer gives up on rewinds to.
pub(crate) struct RecognizerCheckpoint<'a> {
    parser: ParserCheckpoint<'a>,
    ctx: Context,
    strict: bool,
    binding_kind: Option<DeclKind>,
    protos: u32,
    lazy_function_bodies: usize,
    /// The escaped strings and templates listed (`Lexer::escaped_strings`): none read again
    /// is listed anew while these are unchanged.
    escaped: (usize, usize),
}

impl<'a, C: Config> ParserImpl<'a, C> {
    pub(crate) fn recognizer_checkpoint(&mut self) -> RecognizerCheckpoint<'a> {
        RecognizerCheckpoint {
            parser: self.checkpoint(),
            ctx: self.ctx,
            strict: self.strict,
            binding_kind: self.binding_kind,
            protos: self.early.protos,
            lazy_function_bodies: self.lazy_function_bodies.len(),
            escaped: (self.lexer.escaped_strings.len(), self.lexer.escaped_templates.len()),
        }
    }

    pub(crate) fn recognizer_rewind(&mut self, checkpoint: RecognizerCheckpoint<'a>) {
        let RecognizerCheckpoint { parser, ctx, strict, binding_kind, protos, lazy_function_bodies, escaped } = checkpoint;
        self.rewind(parser);
        // The strings and templates read past the checkpoint are read again; left listed, they
        // would read as lying after every body released before them (`escaped_after`).
        let from = self.cur_start();
        if self.lexer.escaped_strings.len() != escaped.0 {
            self.lexer.escaped_strings.retain(|start, _| *start <= from);
        }
        if self.lexer.escaped_templates.len() != escaped.1 {
            self.lexer.escaped_templates.retain(|start, _| *start <= from);
        }
        self.ctx = ctx;
        self.strict = strict;
        self.binding_kind = binding_kind;
        self.early.protos = protos;
        self.lazy_function_bodies.truncate(lazy_function_bodies);
    }

    /// `parse_function_body` while recognizing: the body's directives, and no statements.
    pub(crate) fn recognize_function_body(&mut self) -> ArenaBox<'a, FunctionBody<'a>> {
        let start = self.cur_start();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LCurly);
        let strict = self.strict;
        let ctx = self.ctx;
        self.ctx = ctx.union(Context::Return).difference(Context::TopLevel);
        let mut directives = ArenaVec::new_in(self);
        // The directive prologue, parsed and read as `parse_directives_and_statements` does.
        while self.at(Kind::Str) && self.fatal_error.is_none() {
            match self.parse_statement_list_item(StatementContext::StatementList) {
                Statement::ExpressionStatement(expr)
                    if matches!(&expr.expression, Expression::StringLiteral(string)
                        if expr.span.start == string.span.start) =>
                {
                    let ExpressionStatement { span, expression, .. } = expr.unbox();
                    let Expression::StringLiteral(string) = expression else { unreachable!() };
                    let string = string.unbox();
                    let src = &self.source_text[string.span.start as usize + 1..string.span.end as usize - 1];
                    if src == "use strict" {
                        self.strict = true;
                        self.early.use_strict();
                    }
                    directives.push(Directive::new(span, string, Str::from(src), self));
                }
                _ => break,
            }
        }
        while self.fatal_error.is_none() && !matches!(self.cur_kind(), Kind::RCurly | Kind::Eof | Kind::Undetermined) {
            self.recognize_statement(StatementContext::StatementList);
        }
        self.ctx = ctx;
        self.strict = strict;
        self.expect_closing(Kind::RCurly, opening_span);
        FunctionBody::boxed(self.end_span(start), directives, ArenaVec::new_in(self), self)
    }

    fn recognize_statement(&mut self, stmt_ctx: StatementContext) {
        let checkpoint = self.recognizer_checkpoint();
        if self.rc_statement(stmt_ctx).is_err() {
            self.recognizer_rewind(checkpoint);
            self.parse_statement_list_item(stmt_ctx);
        }
    }

    /// The native stack is too deep to recognize further (`ParserImpl::rc_budget`).
    #[inline]
    fn rc_deep(&self) -> bool {
        !self.rc_budget.as_ref().is_some_and(|budget| budget.admits(0))
    }

    /// Err once a construct handed to the parser stopped it.
    #[inline]
    fn rc_live(&self) -> Rc<()> {
        if self.fatal_error.is_some() { Err(()) } else { Ok(()) }
    }

    /// `eat`, inlined.
    #[inline]
    fn rc_take(&mut self, kind: Kind) -> bool {
        if self.at(kind) {
            self.bump_any();
            true
        } else {
            false
        }
    }

    #[inline]
    fn rc_eat(&mut self, kind: Kind) -> Rc<()> {
        if self.at(kind) {
            self.bump_any();
            Ok(())
        } else {
            Err(())
        }
    }

    #[inline]
    fn rc_asi(&mut self) -> Rc<()> {
        if self.rc_take(Kind::Semicolon) || self.can_insert_semicolon() { Ok(()) } else { Err(()) }
    }

    fn rc_statement(&mut self, stmt_ctx: StatementContext) -> Rc<()> {
        if self.rc_deep() {
            return Err(());
        }
        match self.cur_kind() {
            Kind::LCurly => self.rc_block(),
            Kind::Semicolon => {
                self.bump_any();
                Ok(())
            }
            Kind::If => {
                self.bump_any();
                self.rc_paren_expr()?;
                self.recognize_statement(StatementContext::If);
                if self.rc_take(Kind::Else) {
                    self.recognize_statement(StatementContext::If);
                }
                self.rc_live()
            }
            Kind::Do => {
                self.bump_any();
                self.early.enter_loop();
                self.recognize_statement(StatementContext::Do);
                self.early.exit_loop();
                self.rc_live()?;
                self.rc_eat(Kind::While)?;
                self.rc_paren_expr()?;
                self.bump(Kind::Semicolon);
                Ok(())
            }
            Kind::While => {
                self.bump_any();
                self.rc_paren_expr()?;
                self.early.enter_loop();
                self.recognize_statement(StatementContext::While);
                self.early.exit_loop();
                self.rc_live()
            }
            Kind::For => {
                self.early.enter(ScopeKind::Block);
                self.early.enter_loop();
                let recognized = self.rc_for();
                self.early.exit_loop();
                self.early.exit();
                recognized
            }
            kind @ (Kind::Continue | Kind::Break) => {
                let continues = kind == Kind::Continue;
                self.bump_any();
                if self.can_insert_semicolon() {
                    self.early.jump(continues);
                } else {
                    let kind = self.cur_kind();
                    if !kind.is_label_identifier(self.ctx.has_yield(), self.ctx.has_await()) || self.rc_reserved_here(kind) {
                        return Err(());
                    }
                    let name = self.rc_name();
                    if super::expression::reserved_label(name) {
                        self.early.mark_unsure();
                    }
                    self.early.labeled_jump(name, continues);
                }
                self.rc_asi()
            }
            Kind::Return => {
                if !self.ctx.has_return() {
                    return Err(());
                }
                self.bump_any();
                if !(self.rc_take(Kind::Semicolon) || self.can_insert_semicolon()) {
                    self.rc_expr_in()?;
                    self.rc_asi()?;
                }
                Ok(())
            }
            Kind::Switch => self.rc_switch(),
            Kind::Throw => {
                self.bump_any();
                if self.cur_token().is_on_new_line() {
                    return Err(());
                }
                self.rc_expr()?;
                self.rc_asi()
            }
            Kind::Try => self.rc_try(),
            Kind::Var => {
                self.bump_any();
                self.rc_variable_statement(VariableDeclarationKind::Var, stmt_ctx)
            }
            Kind::Const => {
                self.bump_any();
                self.rc_variable_statement(VariableDeclarationKind::Const, stmt_ctx)
            }
            Kind::Let if !self.cur_token().escaped() && self.lexer.peek_token().kind() != Kind::Colon => {
                if stmt_ctx.is_single_statement() || !self.lexer.peek_token().kind().is_after_let() {
                    return Err(());
                }
                self.bump_any();
                self.rc_variable_statement(VariableDeclarationKind::Let, stmt_ctx)
            }
            Kind::Await if self.is_using_statement() => Err(()),
            Kind::Using if self.is_using_declaration() => Err(()),
            Kind::Function if !stmt_ctx.is_single_statement() => self.rc_function(false),
            Kind::Class if !stmt_ctx.is_single_statement() => self.rc_class(false),
            Kind::Function
            | Kind::Class
            | Kind::Export
            | Kind::Import
            | Kind::At
            | Kind::Async
            | Kind::With
            | Kind::Debugger => {
                self.parse_statement_list_item(stmt_ctx);
                self.rc_live()
            }
            _ => self.rc_expression_or_labeled_statement(),
        }
    }

    fn rc_block(&mut self) -> Rc<()> {
        self.early.enter(ScopeKind::Block);
        self.rc_block_in_scope()?;
        self.early.exit();
        Ok(())
    }

    fn rc_block_in_scope(&mut self) -> Rc<()> {
        self.rc_eat(Kind::LCurly)?;
        while !self.at(Kind::RCurly) {
            if matches!(self.cur_kind(), Kind::Eof | Kind::Undetermined) {
                return Err(());
            }
            self.recognize_statement(StatementContext::StatementList);
            self.rc_live()?;
        }
        self.bump_any();
        Ok(())
    }

    fn rc_switch(&mut self) -> Rc<()> {
        self.bump_any();
        self.rc_paren_expr()?;
        self.rc_eat(Kind::LCurly)?;
        self.early.enter(ScopeKind::Block);
        self.early.enter_switch();
        let recognized = self.rc_switch_cases();
        self.early.exit_switch();
        recognized?;
        self.early.exit();
        Ok(())
    }

    fn rc_switch_cases(&mut self) -> Rc<()> {
        let mut defaults = 0;
        loop {
            match self.cur_kind() {
                Kind::RCurly => break,
                Kind::Case => {
                    self.bump_any();
                    self.rc_expr()?;
                }
                Kind::Default => {
                    self.bump_any();
                    defaults += 1;
                    if defaults > 1 {
                        return Err(());
                    }
                }
                _ => return Err(()),
            }
            self.rc_eat(Kind::Colon)?;
            while !matches!(self.cur_kind(), Kind::Case | Kind::Default | Kind::RCurly | Kind::Eof | Kind::Undetermined) {
                // A `using` declaration may not lie directly in a case.
                if matches!(self.cur_kind(), Kind::Using | Kind::Await) {
                    return Err(());
                }
                self.recognize_statement(StatementContext::StatementList);
                self.rc_live()?;
            }
        }
        self.bump_any();
        Ok(())
    }

    fn rc_try(&mut self) -> Rc<()> {
        self.bump_any();
        self.rc_block()?;
        let handler = self.at(Kind::Catch);
        if handler {
            self.bump_any();
            self.early.enter(ScopeKind::Block);
            if self.rc_take(Kind::LParen) {
                if self.cur_kind().is_binding_identifier() {
                    self.rc_binding_identifier(DeclKind::CatchParam)?;
                } else {
                    let binding_kind = self.binding_kind.replace(DeclKind::Lexical);
                    self.parse_binding_pattern_with_type_annotation();
                    self.binding_kind = binding_kind;
                    self.rc_live()?;
                }
                self.rc_eat(Kind::RParen)?;
            }
            self.rc_block_in_scope()?;
            self.early.exit();
        }
        if self.rc_take(Kind::Finally) {
            self.rc_block()?;
        } else if !handler {
            return Err(());
        }
        Ok(())
    }

    fn rc_variable_statement(&mut self, kind: VariableDeclarationKind, stmt_ctx: StatementContext) -> Rc<()> {
        if stmt_ctx.is_single_statement() && kind.is_lexical() {
            return Err(());
        }
        loop {
            let (pattern, init) = self.rc_declarator(kind)?;
            if !init && (pattern || kind == VariableDeclarationKind::Const) {
                return Err(());
            }
            if !self.rc_take(Kind::Comma) {
                break;
            }
        }
        self.rc_asi()
    }

    /// One declarator: whether it binds a pattern, and whether it has an initializer.
    fn rc_declarator(&mut self, kind: VariableDeclarationKind) -> Rc<(bool, bool)> {
        let declares = if kind == VariableDeclarationKind::Var { DeclKind::Var } else { DeclKind::Lexical };
        let pattern = matches!(self.cur_kind(), Kind::LCurly | Kind::LBrack);
        if pattern {
            let binding_kind = self.binding_kind.replace(declares);
            self.parse_binding_pattern();
            self.binding_kind = binding_kind;
            self.rc_live()?;
        } else {
            self.rc_binding_identifier(declares)?;
        }
        let init = self.rc_take(Kind::Eq);
        if init {
            self.rc_assign()?;
        }
        Ok((pattern, init))
    }

    fn rc_for(&mut self) -> Rc<()> {
        self.bump_any();
        if self.at(Kind::Await) {
            return Err(());
        }
        self.rc_eat(Kind::LParen)?;
        if self.at(Kind::Semicolon) {
            return self.rc_for_loop();
        }
        let declaration = match self.cur_kind() {
            Kind::Var => Some(VariableDeclarationKind::Var),
            Kind::Const => Some(VariableDeclarationKind::Const),
            Kind::Let if !self.cur_token().escaped() && self.lexer.peek_token().kind().is_after_let() => {
                Some(VariableDeclarationKind::Let)
            }
            Kind::Let | Kind::Async | Kind::Using | Kind::Await | Kind::RParen => return Err(()),
            _ => None,
        };
        if let Some(kind) = declaration {
            self.bump_any();
            let ctx = self.ctx;
            self.ctx = ctx.and_in(false);
            let mut count = 0;
            let mut init = false;
            let mut uninitialized_pattern = false;
            loop {
                let (pattern, has_init) = self.rc_declarator(kind)?;
                count += 1;
                init |= has_init;
                uninitialized_pattern |= !has_init && (pattern || kind == VariableDeclarationKind::Const);
                if !self.rc_take(Kind::Comma) {
                    break;
                }
            }
            self.ctx = ctx;
            return match self.cur_kind() {
                kind_in @ (Kind::In | Kind::Of) => {
                    if count > 1 || kind_in == Kind::In && kind.is_lexical() && init {
                        return Err(());
                    }
                    if init {
                        self.early.mark_unsure();
                    }
                    self.rc_for_in_of(kind_in)
                }
                _ if uninitialized_pattern => Err(()),
                _ => self.rc_for_loop(),
            };
        }
        let ctx = self.ctx;
        self.ctx = ctx.and_in(false);
        let init = self.rc_expr()?;
        self.ctx = ctx;
        match self.cur_kind() {
            kind @ (Kind::In | Kind::Of) => {
                match init {
                    Shape::Ident | Shape::ParenIdent => {
                        let name = self.rc_ident;
                        if self.strict && matches!(name.as_str(), "eval" | "arguments") {
                            return Err(());
                        }
                        self.early.assignment_target(name.as_str());
                    }
                    Shape::Member | Shape::ParenMember => {}
                    _ => return Err(()),
                }
                self.rc_for_in_of(kind)
            }
            _ => self.rc_for_loop(),
        }
    }

    fn rc_for_in_of(&mut self, kind: Kind) -> Rc<()> {
        self.bump_any();
        if kind == Kind::In {
            self.rc_expr()?;
        } else {
            self.rc_assign()?;
        }
        self.rc_eat(Kind::RParen)?;
        self.recognize_statement(StatementContext::For);
        self.rc_live()
    }

    /// The rest of a `for (init; test; update)` loop, at the first `;`.
    fn rc_for_loop(&mut self) -> Rc<()> {
        self.rc_eat(Kind::Semicolon)?;
        if !matches!(self.cur_kind(), Kind::Semicolon | Kind::RParen) {
            self.rc_expr_in()?;
        }
        self.rc_eat(Kind::Semicolon)?;
        if !self.at(Kind::RParen) {
            self.rc_expr_in()?;
        }
        self.rc_eat(Kind::RParen)?;
        self.recognize_statement(StatementContext::For);
        self.rc_live()
    }

    fn rc_expression_or_labeled_statement(&mut self) -> Rc<()> {
        let shape = self.rc_expr()?;
        if shape == Shape::Ident
            && self.rc_take(Kind::Colon)
        {
            let name = self.rc_ident;
            self.early.unreference(name.as_str());
            self.early.enter_label(name.as_str(), matches!(self.cur_kind(), Kind::For | Kind::While | Kind::Do));
            self.recognize_statement(StatementContext::Label);
            self.early.exit_label();
            return self.rc_live();
        }
        self.rc_asi()
    }

    fn rc_paren_expr(&mut self) -> Rc<()> {
        self.rc_eat(Kind::LParen)?;
        self.rc_expr()?;
        self.rc_eat(Kind::RParen)
    }

    fn rc_expr_in(&mut self) -> Rc<Shape> {
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true);
        let shape = self.rc_expr()?;
        self.ctx = ctx;
        Ok(shape)
    }

    /// `await` or `yield` as an identifier where its context makes it an error.
    #[inline]
    fn rc_reserved_here(&self, kind: Kind) -> bool {
        kind == Kind::Await && self.ctx.has_await() || kind == Kind::Yield && self.ctx.has_yield()
    }

    /// The current identifier's name, consumed (`parse_identifier_kind`).
    #[inline]
    fn rc_name(&mut self) -> &'a str {
        let token = self.cur_token();
        let name = if token.escaped() {
            self.early.escaped_name();
            self.cur_string()
        } else {
            self.token_source(&token)
        };
        self.advance(Kind::Ident);
        name
    }

    fn rc_binding_identifier(&mut self, declares: DeclKind) -> Rc<()> {
        let kind = self.cur_kind();
        if !kind.is_binding_identifier() || self.rc_reserved_here(kind) {
            return Err(());
        }
        let name = self.rc_name();
        let name = self.ident(name);
        self.early.declare(name, declares);
        Ok(())
    }

    /// A plain function, declaration or expression, whose parameters are identifiers
    /// (`parse_function_impl` and `parse_function` with a body that is not lazy, as no body
    /// inside a recognized one is). Err on a generator or any other parameter.
    fn rc_function(&mut self, expression: bool) -> Rc<()> {
        let start = self.cur_start();
        let checkpoint = self.recognizer_checkpoint();
        if self.rc_function_header(expression).is_err() {
            // Read by the parser instead, its body recognized all the same.
            self.recognizer_rewind(checkpoint);
            if expression {
                self.parse_function_expression(start, false);
            } else {
                self.parse_function_declaration(start, false, StatementContext::StatementList);
            }
            return self.rc_live();
        }
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true).and_await(false).and_yield(false).and_new_target(true);
        self.parse_function_body();
        self.early.exit_function();
        self.ctx = ctx;
        self.rc_live()
    }

    /// A class (`parse_class` in JavaScript), its plain methods recognized here
    /// (`rc_class_method`) and every other element read by the parser.
    fn rc_class(&mut self, expression: bool) -> Rc<()> {
        let strict = std::mem::replace(&mut self.strict, true);
        self.bump_any();
        let binding_kind = self.binding_kind.take();
        let id = (self.cur_kind().is_binding_identifier()
            && !(self.at(Kind::Implements) && self.lexer.peek_token().kind().is_identifier_or_keyword()))
        .then(|| self.parse_binding_identifier().name);
        self.binding_kind = binding_kind;
        self.rc_live()?;
        if let Some(name) = id
            && !expression
        {
            self.early.declare(name, DeclKind::Lexical);
        }
        self.early.enter(crate::early::ScopeKind::Block);
        if let Some(name) = id {
            self.early.declare(name, DeclKind::OwnName);
        }
        let (extends, implements) = self.parse_class_heritage_clause();
        self.rc_live()?;
        if implements.is_some() {
            return Err(());
        }
        self.early.enter_class_body(extends.is_some_and(|extends| !extends.is_empty()));
        self.rc_eat(Kind::LCurly)?;
        loop {
            while self.rc_take(Kind::Semicolon) {}
            if self.at(Kind::RCurly) || self.fatal_error.is_some() {
                break;
            }
            let checkpoint = self.recognizer_checkpoint();
            if self.rc_class_method().is_err() {
                self.recognizer_rewind(checkpoint);
                self.parse_class_element();
                self.rc_live()?;
            }
        }
        self.rc_eat(Kind::RCurly)?;
        self.early.exit_class_body();
        self.early.exit();
        self.strict = strict;
        Ok(())
    }

    /// `[static] [get|set] name(a, b) { ... }`, its name a plain identifier (no keyword,
    /// accessor or modifier word) and its parameters identifiers (`parse_method` with a body that is not
    /// lazy); Err on anything else, which the parser then reads.
    fn rc_class_method(&mut self) -> Rc<()> {
        let named = |p: &mut Self| !p.cur_token().escaped() && p.lexer.peek_token().kind() == Kind::Ident;
        let r#static = self.at(Kind::Static) && named(self);
        if r#static {
            self.bump_any();
        }
        let accessor = (matches!(self.cur_kind(), Kind::Get | Kind::Set) && named(self)).then(|| self.cur_kind());
        if accessor.is_some() {
            self.bump_any();
        }
        let home = if !r#static && accessor.is_none() && self.at(Kind::Constructor) {
            // `parse_constructor_declaration`.
            self.bump_any();
            if !self.at(Kind::LParen) {
                return Err(());
            }
            self.early.method_home(true)
        } else {
            if !self.at(Kind::Ident) {
                return Err(());
            }
            let name = self.rc_name();
            if r#static && name == "prototype" || !self.at(Kind::LParen) {
                return Err(());
            }
            crate::early::Home::Method
        };
        self.early.enter_function(Some(home));
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true).and_await(false).and_yield(false).and_new_target(true);
        self.bump_any();
        // A getter takes no parameter and a setter exactly one (`check_method_definition_accessor`).
        if accessor != Some(Kind::Get) && !self.at(Kind::RParen) {
            self.early.set_in_params(true);
            loop {
                self.rc_binding_identifier(DeclKind::Param)?;
                if accessor.is_some() || !self.rc_take(Kind::Comma) || self.at(Kind::RParen) {
                    break;
                }
            }
            self.early.set_in_params(false);
        } else if accessor == Some(Kind::Set) {
            return Err(());
        }
        self.rc_eat(Kind::RParen)?;
        self.early.set_simple_params(true);
        if !self.at(Kind::LCurly) {
            return Err(());
        }
        // Marked once nothing can rewind it: a rewind keeps the innermost class's mark.
        if matches!(home, crate::early::Home::BaseConstructor | crate::early::Home::DerivedConstructor) {
            self.early.constructor();
        }
        self.parse_function_body();
        self.early.exit_function();
        self.ctx = ctx;
        self.rc_live()
    }

    /// `function`, its name and identifier parameters, up to the body's `{`, scope entered;
    /// Err on anything else.
    fn rc_function_header(&mut self, expression: bool) -> Rc<()> {
        self.bump_any();
        if self.at(Kind::Star) {
            return Err(());
        }
        let kind = self.cur_kind();
        let ctx = self.ctx;
        if kind.is_binding_identifier() {
            if expression {
                self.ctx = ctx.and_await(false).and_yield(false);
            }
            let reserved = self.rc_reserved_here(kind);
            self.ctx = ctx;
            if reserved {
                return Err(());
            }
            let name = self.rc_name();
            let name = self.ident(name);
            if expression {
                self.early.enter_function(Some(crate::early::Home::Plain));
                self.early.declare(name, DeclKind::OwnName);
            } else {
                self.declare_function_name(name, false);
                self.early.enter_function(Some(crate::early::Home::Plain));
            }
        } else if expression {
            self.early.enter_function(Some(crate::early::Home::Plain));
        } else {
            return Err(());
        }
        self.ctx = ctx.and_in(true).and_await(false).and_yield(false).and_new_target(true);
        self.rc_eat(Kind::LParen)?;
        if !self.at(Kind::RParen) {
            self.early.set_in_params(true);
            loop {
                self.rc_binding_identifier(DeclKind::Param)?;
                if !self.rc_take(Kind::Comma) || self.at(Kind::RParen) {
                    break;
                }
            }
            self.early.set_in_params(false);
        }
        self.rc_eat(Kind::RParen)?;
        self.early.set_simple_params(true);
        self.ctx = ctx;
        if !self.at(Kind::LCurly) {
            return Err(());
        }
        Ok(())
    }

    fn rc_expr(&mut self) -> Rc<Shape> {
        if self.ctx.has_decorator() {
            return Err(());
        }
        let shape = self.rc_assign()?;
        if !self.at(Kind::Comma) {
            return Ok(shape);
        }
        while self.rc_take(Kind::Comma) {
            self.rc_assign()?;
        }
        Ok(Shape::Other)
    }

    fn rc_assign(&mut self) -> Rc<Shape> {
        // The stack depth is checked where each operand begins (`rc_binary`).
        let kind = self.cur_kind();
        if matches!(kind, Kind::Yield | Kind::LParen | Kind::Async | Kind::LBrack | Kind::LCurly) {
            return self.rc_assign_led(kind);
        }
        let shape = self.rc_binary()?;
        self.rc_assign_rest(shape, None)
    }

    /// `rc_assign` where the first token may begin something else: `yield`, an arrow
    /// function, or a pattern.
    #[inline(never)]
    fn rc_assign_led(&mut self, kind: Kind) -> Rc<Shape> {
        if kind == Kind::Yield {
            self.parse_assignment_expression_or_higher();
            self.rc_live()?;
            return Ok(Shape::Other);
        }
        if matches!(kind, Kind::LParen | Kind::Async) {
            if self.try_parse_parenthesized_arrow_function_expression(true).is_some() {
                self.rc_live()?;
                return Ok(Shape::Other);
            }
            if kind == Kind::Async && self.try_parse_async_simple_arrow_function_expression(true).is_some() {
                self.rc_live()?;
                return Ok(Shape::Other);
            }
            self.rc_live()?;
        }
        // An object or array literal is a pattern if `=` follows: the parser reads it then.
        let pattern = matches!(kind, Kind::LBrack | Kind::LCurly).then(|| self.recognizer_checkpoint());
        let shape = self.rc_binary()?;
        self.rc_assign_rest(shape, pattern)
    }

    /// What follows an assignment expression's conditional operand: `=>`, an assignment
    /// operator or `?`.
    #[inline(always)]
    fn rc_assign_rest(&mut self, shape: Shape, pattern: Option<RecognizerCheckpoint<'a>>) -> Rc<Shape> {
        let kind = self.cur_kind();
        if kind == Kind::Arrow {
            if shape != Shape::Ident {
                return Err(());
            }
            let name = self.rc_ident;
            if self.cur_token().is_on_new_line() {
                return Err(());
            }
            self.early.unreference(name.as_str());
            self.early.enter_function(None);
            self.early.declare(name, DeclKind::Param);
            self.bump_any();
            return self.rc_arrow_body(false);
        }
        if kind.is_assignment_operator() {
            match shape {
                Shape::Ident | Shape::ParenIdent => self.early.assignment_target(self.rc_ident.as_str()),
                Shape::Member | Shape::ParenMember => {}
                Shape::Literal if let Some(checkpoint) = pattern => {
                    self.recognizer_rewind(checkpoint);
                    self.parse_assignment_expression_or_higher();
                    self.rc_live()?;
                    return Ok(Shape::Other);
                }
                _ => return Err(()),
            }
            self.bump_any();
            self.rc_assign()?;
            return Ok(Shape::Other);
        }
        if self.rc_take(Kind::Question) {
            let ctx = self.ctx;
            self.ctx = ctx.and_in(true);
            self.rc_assign()?;
            self.ctx = ctx;
            self.rc_eat(Kind::Colon)?;
            self.rc_assign()?;
            return Ok(Shape::Other);
        }
        Ok(shape)
    }

    /// An arrow function's body after `=>`, its scope entered (`parse_arrow_function_expression_body`).
    fn rc_arrow_body(&mut self, r#async: bool) -> Rc<Shape> {
        let ctx = self.ctx;
        self.ctx = ctx.and_await(r#async).and_yield(false);
        if self.at(Kind::LCurly) {
            self.ctx = self.ctx.and_in(true);
            self.recognize_function_body();
            self.rc_live()?;
        } else {
            self.ctx = self.ctx.difference(Context::TopLevel);
            self.rc_assign()?;
        }
        self.ctx = ctx;
        self.early.exit_function();
        Ok(Shape::Other)
    }

    /// The binary expression levels down to the primary: `operand (operator operand)*`, each
    /// operand `prefix* primary suffix* postfix?`, read in one loop as no tree is built. The
    /// rules precedence decides are kept: `??` mixes with neither `&&` nor `||` unparenthesized,
    /// and the left of `**` is no unary expression.
    fn rc_binary(&mut self) -> Rc<Shape> {
        let mut single = true;
        let mut logical = false;
        let mut coalesce = false;
        loop {
            if self.rc_deep() {
                return Err(());
            }
            // Prefix operators: `++`/`--` only last, its operand a simple target.
            let mut prefixed = false;
            let mut update = false;
            let mut delete = false;
            loop {
                let kind = self.cur_kind();
                if kind.is_unary_operator() {
                    delete = kind == Kind::Delete;
                } else if kind == Kind::Await {
                    if !self.ctx.has_await() {
                        if self.is_unambiguous_await() {
                            return Err(());
                        }
                        break;
                    }
                    self.early.await_or_yield(true);
                    delete = false;
                } else if kind.is_update_operator() {
                    self.bump_any();
                    update = true;
                    break;
                } else {
                    break;
                }
                prefixed = true;
                self.bump_any();
            }
            let kind = self.cur_kind();
            if update && (kind.is_unary_operator() || kind.is_update_operator() || kind == Kind::Await) || kind == Kind::LAngle {
                return Err(());
            }
            if kind == Kind::PrivateIdentifier {
                return Err(());
            }
            let mut operand = self.rc_lhs()?;
            if update {
                self.rc_simple_target(operand)?;
                operand = Shape::Other;
            }
            if delete && matches!(operand, Shape::Ident | Shape::ParenIdent) {
                self.early.mark_unsure();
            }
            if self.cur_kind().is_update_operator() && !self.cur_token().is_on_new_line() {
                if update {
                    return Err(());
                }
                self.rc_simple_target(operand)?;
                self.bump_any();
                operand = Shape::Other;
            }
            let kind = self.re_lex_right_angle();
            if !(kind.is_binary_operator() || kind.is_logical_operator()) || kind == Kind::In && !self.ctx.has_in() {
                if matches!(kind, Kind::As | Kind::Satisfies) && !self.cur_token().is_on_new_line() {
                    return Err(());
                }
                return Ok(if single && !prefixed && !update { operand } else { Shape::Other });
            }
            match kind {
                Kind::Star2 if prefixed => return Err(()),
                Kind::Question2 => coalesce = true,
                Kind::Amp2 | Kind::Pipe2 => logical = true,
                _ => {}
            }
            if coalesce && logical {
                return Err(());
            }
            self.bump_any();
            single = false;
        }
    }

    fn rc_simple_target(&mut self, shape: Shape) -> Rc<()> {
        match shape {
            Shape::Ident | Shape::ParenIdent => {
                self.early.assignment_target(self.rc_ident.as_str());
                Ok(())
            }
            Shape::Member | Shape::ParenMember => Ok(()),
            _ => Err(()),
        }
    }

    /// A primary expression and its member, call and tagged template suffixes.
    fn rc_lhs(&mut self) -> Rc<Shape> {
        let mut shape = self.rc_primary()?;
        let mut chain = false;
        loop {
            match self.cur_kind() {
                Kind::Dot => {
                    self.bump_any();
                    self.rc_identifier_name()?;
                    shape = Shape::Member;
                }
                Kind::LBrack => {
                    self.rc_computed_member()?;
                    shape = Shape::Member;
                }
                Kind::LParen => {
                    self.rc_arguments()?;
                    shape = Shape::Other;
                }
                Kind::QuestionDot => {
                    self.bump_any();
                    chain = true;
                    match self.cur_kind() {
                        Kind::LParen => {
                            self.rc_arguments()?;
                            shape = Shape::Other;
                        }
                        Kind::LBrack => {
                            self.rc_computed_member()?;
                            shape = Shape::Member;
                        }
                        kind if kind.is_identifier_or_keyword() => {
                            self.rc_name();
                            shape = Shape::Member;
                        }
                        _ => return Err(()),
                    }
                }
                kind if kind.is_template_start_of_tagged_template() => {
                    if chain {
                        return Err(());
                    }
                    self.parse_template_literal(true);
                    self.rc_live()?;
                    shape = Shape::Other;
                }
                _ => break,
            }
        }
        Ok(if chain { Shape::Chain } else { shape })
    }

    /// The member suffixes of a `new` expression's callee.
    fn rc_member_rest(&mut self, mut shape: Shape) -> Rc<Shape> {
        loop {
            match self.cur_kind() {
                Kind::Dot => {
                    self.bump_any();
                    self.rc_identifier_name()?;
                    shape = Shape::Member;
                }
                Kind::LBrack => {
                    self.rc_computed_member()?;
                    shape = Shape::Member;
                }
                Kind::QuestionDot => return Err(()),
                kind if kind.is_template_start_of_tagged_template() => {
                    self.parse_template_literal(true);
                    self.rc_live()?;
                    shape = Shape::Other;
                }
                _ => return Ok(shape),
            }
        }
    }

    fn rc_identifier_name(&mut self) -> Rc<()> {
        if !self.cur_kind().is_identifier_name() {
            return Err(());
        }
        self.rc_name();
        Ok(())
    }

    fn rc_computed_member(&mut self) -> Rc<()> {
        self.bump_any();
        self.rc_expr_in()?;
        self.rc_eat(Kind::RBrack)
    }

    fn rc_arguments(&mut self) -> Rc<()> {
        self.bump_any();
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true);
        while !self.at(Kind::RParen) {
            self.bump(Kind::Dot3);
            self.rc_assign()?;
            if !self.rc_take(Kind::Comma) {
                break;
            }
        }
        self.ctx = ctx;
        self.rc_eat(Kind::RParen)
    }

    fn rc_primary(&mut self) -> Rc<Shape> {
        match self.cur_kind() {
            Kind::Ident => self.rc_identifier_reference(),
            Kind::This | Kind::True | Kind::False | Kind::Null => {
                self.bump_any();
                Ok(Shape::Other)
            }
            Kind::Str => {
                if self.cur_token().escaped() && legacy_escape(self.cur_src()) {
                    self.early.mark_unsure();
                }
                self.bump_any();
                Ok(Shape::Other)
            }
            Kind::LParen => self.rc_parenthesized(),
            Kind::LBrack => self.rc_array(),
            Kind::LCurly => self.rc_object(),
            Kind::New => self.rc_new(),
            Kind::Super => {
                self.bump_any();
                if !matches!(self.cur_kind(), Kind::Dot | Kind::LBrack | Kind::LParen) {
                    return Err(());
                }
                self.early.super_expression(self.at(Kind::LParen));
                Ok(Shape::Super)
            }
            Kind::Async if !self.at_function_with_async() => self.rc_identifier_reference(),
            Kind::Function => {
                self.rc_function(true)?;
                Ok(Shape::Other)
            }
            Kind::Class => {
                self.rc_class(true)?;
                Ok(Shape::Other)
            }
            Kind::Async
            | Kind::NoSubstitutionTemplate
            | Kind::TemplateHead
            | Kind::Import
            | Kind::Slash
            | Kind::SlashEq => {
                self.parse_primary_expression();
                self.rc_live()?;
                Ok(Shape::Other)
            }
            // A plain decimal reads as the parser reads it: no separator, no leading zero.
            Kind::Decimal | Kind::Float
                if !self.cur_token().has_separator() && {
                    let src = self.cur_src().as_bytes();
                    src.len() < 2 || src[0] != b'0' || !src[1].is_ascii_digit()
                } =>
            {
                self.bump_any();
                Ok(Shape::Other)
            }
            kind if kind.is_number() => {
                self.parse_literal_expression();
                self.rc_live()?;
                Ok(Shape::Other)
            }
            Kind::Percent | Kind::At | Kind::PrivateIdentifier | Kind::LAngle => Err(()),
            _ => self.rc_identifier_reference(),
        }
    }

    fn rc_identifier_reference(&mut self) -> Rc<Shape> {
        let kind = self.cur_kind();
        if !kind.is_identifier_reference(false, false) || self.rc_reserved_here(kind) {
            return Err(());
        }
        if kind == Kind::Await {
            self.state.encountered_await_identifier = true;
        }
        let name = self.rc_name();
        let name = self.ident(name);
        // Words a semantic rule reads lex as keywords unless escaped (`rc_name` loses those).
        self.early.reference_word(name, kind != Kind::Ident);
        self.rc_ident = name;
        Ok(Shape::Ident)
    }

    fn rc_parenthesized(&mut self) -> Rc<Shape> {
        self.bump_any();
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true).and_decorator(false);
        if self.at(Kind::RParen) {
            return Err(());
        }
        let first = self.rc_assign()?;
        let mut single = true;
        while self.rc_take(Kind::Comma) {
            if self.at(Kind::RParen) {
                return Err(());
            }
            self.rc_assign()?;
            single = false;
        }
        self.ctx = ctx;
        self.rc_eat(Kind::RParen)?;
        Ok(match first {
            Shape::Ident if single => Shape::ParenIdent,
            Shape::Member if single => Shape::ParenMember,
            _ => Shape::Other,
        })
    }

    fn rc_array(&mut self) -> Rc<Shape> {
        self.bump_any();
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true);
        loop {
            match self.cur_kind() {
                Kind::RBrack => break,
                Kind::Comma => {
                    self.bump_any();
                    continue;
                }
                _ => {
                    self.bump(Kind::Dot3);
                    self.rc_assign()?;
                }
            }
            if !self.at(Kind::RBrack) {
                self.rc_eat(Kind::Comma)?;
            }
        }
        self.ctx = ctx;
        self.bump_any();
        Ok(Shape::Literal)
    }

    fn rc_object(&mut self) -> Rc<Shape> {
        self.bump_any();
        let protos = std::mem::take(&mut self.early.protos);
        let ctx = self.ctx;
        self.ctx = ctx.and_in(true);
        while !self.at(Kind::RCurly) {
            let kind = self.cur_kind();
            let next = if kind == Kind::Dot3 { Kind::Dot3 } else { self.lexer.peek_token().kind() };
            if kind == Kind::Dot3 {
                self.bump_any();
                self.rc_assign()?;
            } else if next == Kind::Colon && (kind == Kind::Str || kind.is_identifier_name()) {
                let proto = if kind == Kind::Str {
                    if self.cur_token().escaped() && legacy_escape(self.cur_src()) {
                        self.early.mark_unsure();
                    }
                    let proto = self.cur_string() == "__proto__";
                    self.bump_any();
                    proto
                } else {
                    self.rc_name() == "__proto__"
                };
                if proto && self.early.active() {
                    self.early.protos += 1;
                }
                self.bump_any();
                self.rc_assign()?;
            } else if kind == Kind::Ident && matches!(next, Kind::Comma | Kind::RCurly) {
                let name = self.rc_name();
                let name = self.ident(name);
                self.early.reference_word(name, false);
            } else {
                self.parse_object_expression_property();
                self.rc_live()?;
            }
            if !self.at(Kind::RCurly) {
                self.rc_eat(Kind::Comma)?;
            }
        }
        self.ctx = ctx;
        self.bump_any();
        if std::mem::replace(&mut self.early.protos, protos) >= 2 {
            self.early.mark_unsure();
        }
        Ok(Shape::Literal)
    }

    fn rc_new(&mut self) -> Rc<Shape> {
        if self.rc_deep() {
            return Err(());
        }
        self.bump_any();
        if self.rc_take(Kind::Dot) {
            if !self.at(Kind::Target) || !self.ctx.has_new_target() {
                return Err(());
            }
            self.bump_any();
            return Ok(Shape::Other);
        }
        if self.at(Kind::Import) {
            return Err(());
        }
        let callee = self.rc_primary()?;
        let callee = self.rc_member_rest(callee)?;
        if callee == Shape::Super {
            return Err(());
        }
        if self.at(Kind::LParen) {
            self.rc_arguments()?;
        }
        Ok(Shape::Other)
    }
}
