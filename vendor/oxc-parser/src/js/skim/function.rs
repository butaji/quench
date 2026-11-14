//! Functions inside a skimmed body: declarations, expressions, methods and arrows.
use super::binding::DeclKind;
use super::expr::Form;
use super::{Flavor, FnCtx, Seen, Skim, is_plain_identifier, is_restricted};
use crate::{ParserConfig as Config, lexer::Kind};

/// What a parameter list came to.
pub(super) struct ParamList {
    count: usize,
    /// The last is a rest parameter.
    rest: bool,
    /// Every parameter is a plain identifier.
    simple: bool,
}

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// `function name(...) { ... }`, its name declared where it stands.
    pub(super) fn function_declaration(&mut self, is_async: bool) -> Seen {
        self.bump();
        let generator = self.eat(Kind::Star);
        let name = self.identifier()?;
        if is_restricted(name) {
            return None;
        }
        self.scopes.declare_function(name)?;
        self.scopes.enter(true);
        self.with_ctx(FnCtx::function(Flavor { generator, is_async }), Self::function_rest)
    }

    /// `function name?(...) { ... }`, its name bound inside itself.
    pub(super) fn function_expression(&mut self, is_async: bool) -> Seen<Form> {
        self.bump();
        let generator = self.eat(Kind::Star);
        self.scopes.enter(true);
        if is_plain_identifier(self.kind()) {
            let name = self.identifier()?;
            if is_restricted(name) {
                return None;
            }
            self.scopes.declare_name(name);
        }
        self.with_ctx(FnCtx::function(Flavor { generator, is_async }), Self::function_rest)?;
        Some(Form::Other)
    }

    /// A method: `(...) { ... }` after its key; `super(...)` is valid in a derived constructor.
    pub(super) fn method(&mut self, super_call: bool, flavor: Flavor) -> Seen {
        self.scopes.enter(true);
        self.with_ctx(FnCtx::method(super_call, flavor), Self::function_rest)
    }

    /// The parameters and the body of an accessor: none for a getter, one for a setter.
    pub(super) fn accessor_rest(&mut self, setter: bool) -> Seen {
        self.scopes.enter(true);
        self.with_ctx(FnCtx::method(false, Flavor::default()), |this| {
            let params = this.params()?;
            if params.rest || params.count != usize::from(setter) {
                return None;
            }
            this.function_end(params.simple)
        })
    }

    /// The parameters and the body of the function whose scope is entered; leaves the scope.
    fn function_rest(&mut self) -> Seen {
        let params = self.params()?;
        self.function_end(params.simple)
    }

    fn function_end(&mut self, simple_params: bool) -> Seen {
        self.function_body(simple_params)?;
        self.scopes.exit();
        Some(())
    }

    /// `( parameters ) { ... }`, or `( parameters ) => { ... }` of an arrow, read from the head;
    /// the function's scope is entered. A function nested in a parameter's default does not
    /// give the function its directives.
    pub(super) fn function_from_head(&mut self, arrow: bool) -> Seen {
        self.directives_read = true;
        let params = self.params()?;
        if arrow && !self.arrow_follows() {
            return None;
        }
        self.directives_read = false;
        self.function_body(params.simple)
    }

    /// `( parameter, ... )`.
    pub(super) fn params(&mut self) -> Seen<ParamList> {
        let ctx = FnCtx { generator: false, is_async: false, ..self.ctx };
        self.with_ctx(ctx, Self::param_list)
    }

    fn param_list(&mut self) -> Seen<ParamList> {
        self.expect(Kind::LParen)?;
        let mut list = ParamList { count: 0, rest: false, simple: true };
        while self.kind() != Kind::RParen {
            list.rest = self.eat(Kind::Dot3);
            let simple = if list.rest { self.binding_target(DeclKind::Param)?; false } else { self.binding_element(DeclKind::Param)? };
            list.simple &= simple;
            list.count += 1;
            if list.rest {
                break;
            }
            if self.kind() != Kind::RParen {
                self.expect(Kind::Comma)?;
            }
        }
        self.expect(Kind::RParen)?;
        Some(list)
    }

    /// `{ statements }` of a function, whose labels, loops and `in` are its own.
    pub(super) fn function_body(&mut self, simple_params: bool) -> Seen {
        self.expect(Kind::LCurly)?;
        let strict = self.strict;
        let flow = std::mem::take(&mut self.flow);
        let pending = std::mem::take(&mut self.pending_cover);
        let seen = self.directives(simple_params).and_then(|()| self.nested(Self::statements));
        self.flow = flow;
        self.pending_cover = pending;
        self.strict = strict;
        seen?;
        self.expect(Kind::RCurly)
    }

    /// The directive prologue: `"use strict"` makes the function strict, which its parameters
    /// must allow by being plain. A string that may or may not be a directive is not decided.
    fn directives(&mut self, simple_params: bool) -> Seen {
        let own = !std::mem::replace(&mut self.directives_read, true);
        while self.kind() == Kind::Str {
            let next = self.p.lexer.peek_token().kind();
            let strict = matches!(self.p.cur_src(), "\"use strict\"" | "'use strict'");
            if !matches!(next, Kind::Semicolon | Kind::RCurly) {
                return (!strict).then_some(());
            }
            if strict && !simple_params {
                return None;
            }
            self.strict |= strict;
            let (start, literal_end) = (self.p.cur_start(), self.p.cur_token().end());
            if own && self.p.cur_src().contains('\\') {
                return None;
            }
            self.string_literal()?;
            self.eat(Kind::Semicolon);
            if own {
                self.directives.push((start, literal_end, self.p.prev_token_end));
            }
        }
        Some(())
    }

    /// `async (parameters) => body` or `async name => body`, at `async`; otherwise the parse is
    /// as it was.
    pub(super) fn async_arrow(&mut self) -> Seen<bool> {
        let next = self.p.lexer.peek_token();
        if next.is_on_new_line() {
            return Some(false);
        }
        match next.kind() {
            Kind::LParen => self.async_parenthesized_arrow(),
            kind if is_plain_identifier(kind) => self.async_identifier_arrow(),
            _ => Some(false),
        }
    }

    fn async_parenthesized_arrow(&mut self) -> Seen<bool> {
        let checkpoint = self.p.checkpoint();
        self.bump();
        if self.try_arrow(true)? {
            return Some(true);
        }
        self.p.rewind(checkpoint);
        Some(false)
    }

    /// `async name => body`: nothing but an arrow may follow the name.
    fn async_identifier_arrow(&mut self) -> Seen<bool> {
        self.bump();
        let name = self.identifier()?;
        if is_restricted(name) || self.kind() != Kind::Arrow || self.newline() {
            return None;
        }
        self.bump();
        self.scopes.enter(true);
        self.scopes.declare_param(name)?;
        self.arrow_body(true, true)?;
        Some(true)
    }

    /// An arrow function starting at `(`, when the parenthesized text is its parameters and
    /// `=>` follows; otherwise the parse is as it was.
    pub(super) fn try_arrow(&mut self, is_async: bool) -> Seen<bool> {
        let checkpoint = self.p.checkpoint();
        let mark = self.scopes.mark();
        let pending = self.pending_cover;
        self.scopes.enter(true);
        if let Some(params) = self.params()
            && self.arrow_follows()
        {
            self.arrow_body(params.simple, is_async)?;
            return Some(true);
        }
        self.p.rewind(checkpoint);
        self.scopes.reset(mark);
        self.pending_cover = pending;
        Some(false)
    }

    /// `=>` on the line of the parameters, read.
    fn arrow_follows(&mut self) -> bool {
        let follows = self.kind() == Kind::Arrow && !self.newline();
        if follows {
            self.bump();
        }
        follows
    }

    /// `name => body`, `name` having been read as a reference.
    pub(super) fn identifier_arrow(&mut self) -> Seen<Form> {
        let name = self.last_ident;
        if is_restricted(name) || self.scopes.refs.pop() != Some(name) {
            return None;
        }
        self.scopes.enter(true);
        self.scopes.declare_param(name)?;
        self.bump();
        self.arrow_body(true, false)?;
        Some(Form::Arrow)
    }

    /// The body of the arrow whose scope is entered: a block, or an assignment expression.
    fn arrow_body(&mut self, simple_params: bool, is_async: bool) -> Seen {
        let ctx = FnCtx { generator: false, is_async, ..self.ctx };
        self.with_ctx(ctx, |this| this.arrow_body_in(simple_params))
    }

    fn arrow_body_in(&mut self, simple_params: bool) -> Seen {
        if self.kind() == Kind::LCurly {
            self.function_body(simple_params)?;
        } else {
            self.assignment()?;
        }
        self.scopes.exit();
        Some(())
    }
}
