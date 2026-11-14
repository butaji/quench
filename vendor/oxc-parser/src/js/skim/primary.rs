//! Primary expressions and the member, call and template suffixes of a skimmed body.
use super::expr::Form;
use super::{Seen, Skim, is_plain_identifier};
use crate::{ParserConfig as Config, lexer::Kind};
use oxc_ast::ast::RegExpFlags;

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// A primary expression with its member, call and template suffixes.
    pub(super) fn lhs(&mut self) -> Seen<Form> {
        let mut form = match self.kind() {
            Kind::New => self.deeper(Self::new_expression)?,
            Kind::Super => self.super_expression()?,
            _ => self.primary()?,
        };
        let mut chain = false;
        loop {
            form = match self.kind() {
                Kind::Dot => {
                    self.bump();
                    self.member_name()?;
                    if chain { Form::Chain } else { Form::Member }
                }
                Kind::LBrack => {
                    self.computed_member()?;
                    if chain { Form::Chain } else { Form::Member }
                }
                Kind::LParen => {
                    self.arguments()?;
                    if chain { Form::Chain } else { Form::Call }
                }
                Kind::QuestionDot => {
                    self.optional_link()?;
                    chain = true;
                    Form::Chain
                }
                Kind::NoSubstitutionTemplate | Kind::TemplateHead if !chain => {
                    self.template(true)?;
                    Form::Other
                }
                _ => return Some(form),
            };
        }
    }

    /// `super`, which only a member access or a call may follow, where each is valid.
    fn super_expression(&mut self) -> Seen<Form> {
        self.bump();
        let valid = match self.kind() {
            Kind::LParen => self.ctx.super_call,
            Kind::Dot | Kind::LBrack => self.ctx.super_property,
            _ => false,
        };
        valid.then_some(Form::Other)
    }

    /// The name after `.`: any name but a private one.
    fn member_name(&mut self) -> Seen {
        self.kind().is_identifier_name().then(|| self.bump())
    }

    fn computed_member(&mut self) -> Seen {
        self.bump();
        self.nested(Self::expression)?;
        self.expect(Kind::RBrack)
    }

    /// `?.` and what follows it: a name, an index or a call.
    fn optional_link(&mut self) -> Seen {
        self.bump();
        match self.kind() {
            Kind::LParen => self.arguments(),
            Kind::LBrack => self.computed_member(),
            _ => self.member_name(),
        }
    }

    /// Runs `f` where `in` is an operator again and no assignment is about to be a pattern.
    pub(super) fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Seen<T>) -> Seen<T> {
        let no_in = std::mem::replace(&mut self.no_in, false);
        let seen = f(self);
        self.no_in = no_in;
        seen
    }

    /// `( argument, ... )`.
    fn arguments(&mut self) -> Seen {
        self.bump();
        while self.kind() != Kind::RParen {
            self.eat(Kind::Dot3);
            self.nested(Self::assignment)?;
            if self.kind() != Kind::RParen {
                self.expect(Kind::Comma)?;
            }
        }
        self.bump();
        Some(())
    }

    /// `new.target`, or `new callee` with its arguments if any.
    fn new_expression(&mut self) -> Seen<Form> {
        self.bump();
        if self.eat(Kind::Dot) {
            self.expect(Kind::Target)?;
            return Some(Form::Other);
        }
        if self.kind() == Kind::New {
            self.deeper(Self::new_expression)?;
        } else {
            self.primary()?;
        }
        self.new_callee_suffixes()?;
        if self.kind() == Kind::LParen {
            self.arguments()?;
        }
        Some(Form::Other)
    }

    /// The member accesses of a `new` callee: no calls, and no optional chain.
    fn new_callee_suffixes(&mut self) -> Seen {
        loop {
            match self.kind() {
                Kind::Dot => {
                    self.bump();
                    self.member_name()?;
                }
                Kind::LBrack => self.computed_member()?,
                Kind::NoSubstitutionTemplate | Kind::TemplateHead => self.template(true)?,
                Kind::QuestionDot => return None,
                _ => return Some(()),
            }
        }
    }

    pub(super) fn primary(&mut self) -> Seen<Form> {
        let kind = self.kind();
        match kind {
            Kind::Async => self.async_primary(),
            _ if is_plain_identifier(kind) => self.identifier_reference(),
            Kind::This | Kind::Null | Kind::True | Kind::False => {
                self.bump();
                Some(Form::Other)
            }
            Kind::Str => self.string_literal(),
            _ if kind.is_number() => self.number_literal(),
            Kind::Slash | Kind::SlashEq => self.regexp(),
            Kind::NoSubstitutionTemplate | Kind::TemplateHead => self.template(false).map(|()| Form::Other),
            Kind::LParen => self.parenthesized(),
            Kind::LBrack => self.deeper(Self::array_literal),
            Kind::LCurly => self.deeper(Self::object_literal),
            Kind::Function => self.deeper(|this| this.function_expression(false)),
            Kind::Class => self.deeper(Self::class_expression),
            _ => None,
        }
    }

    /// `async function ...`, or `async` as a name.
    fn async_primary(&mut self) -> Seen<Form> {
        if self.async_function_ahead() {
            self.bump();
            return self.deeper(|this| this.function_expression(true));
        }
        if self.p.cur_token().escaped() {
            return None;
        }
        let name = self.p.cur_src();
        self.bump();
        self.scopes.reference(name);
        self.last_ident = name;
        Some(Form::Ident)
    }

    /// `async` is followed by `function` on its line.
    pub(super) fn async_function_ahead(&mut self) -> bool {
        let next = self.p.lexer.peek_token();
        self.kind() == Kind::Async && next.kind() == Kind::Function && !next.is_on_new_line()
    }

    fn identifier_reference(&mut self) -> Seen<Form> {
        let name = self.identifier()?;
        if self.ctx.field_init && name == "arguments" {
            return None;
        }
        self.scopes.reference(name);
        self.last_ident = name;
        Some(Form::Ident)
    }

    fn parenthesized(&mut self) -> Seen<Form> {
        self.bump();
        let inner = self.nested(Self::expression)?;
        self.expect(Kind::RParen)?;
        Some(if inner.is_simple_target() { Form::ParenTarget } else { Form::Paren })
    }

    /// A string: not one with a legacy octal escape.
    pub(super) fn string_literal(&mut self) -> Seen<Form> {
        let text = self.p.cur_src();
        if text.contains('\\') && has_octal_escape(text) {
            return None;
        }
        self.bump();
        Some(Form::Other)
    }

    /// A number: not a legacy octal or a decimal with a leading zero.
    pub(super) fn number_literal(&mut self) -> Seen<Form> {
        if has_leading_zero(self.p.cur_src()) {
            return None;
        }
        self.bump();
        Some(Form::Other)
    }

    /// A regular expression literal, listed for the validation of its pattern.
    fn regexp(&mut self) -> Seen<Form> {
        let (_, flags, flags_error) = self.p.read_regex();
        if flags_error || flags.contains(RegExpFlags::U | RegExpFlags::V) {
            return None;
        }
        let span = self.p.cur_token().span();
        self.p.regexp_literals.push(span);
        self.bump();
        Some(Form::Other)
    }

    /// A template, whose escapes are all valid unless it is tagged.
    pub(super) fn template(&mut self, tagged: bool) -> Seen {
        loop {
            let token = self.p.cur_token();
            if token.escaped() && !tagged && self.p.cur_template_string().is_none() {
                return None;
            }
            self.bump();
            if matches!(token.kind(), Kind::NoSubstitutionTemplate | Kind::TemplateTail) {
                return Some(());
            }
            self.nested(Self::expression)?;
            self.p.re_lex_template_substitution_tail();
            if !matches!(self.kind(), Kind::TemplateMiddle | Kind::TemplateTail) {
                return None;
            }
        }
    }
}

/// A string literal's text has `\0` followed by a digit, or `\1` to `\9`.
fn has_octal_escape(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at + 1 < bytes.len() {
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
        let digit_after = bytes.get(at + 2).is_some_and(u8::is_ascii_digit);
        if matches!(bytes[at + 1], b'1'..=b'9') || bytes[at + 1] == b'0' && digit_after {
            return true;
        }
        at += 2;
    }
    false
}

/// A numeric literal's text starts with `0` and a digit or a separator.
fn has_leading_zero(text: &str) -> bool {
    matches!(text.as_bytes(), [b'0', next, ..] if next.is_ascii_digit() || *next == b'_')
}
