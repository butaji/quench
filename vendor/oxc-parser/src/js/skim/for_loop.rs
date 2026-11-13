//! `for` statements of a skimmed body.
use super::binding::DeclKind;
use super::expr::Form;
use super::{Seen, Skim};
use crate::{ParserConfig as Config, lexer::Kind};

/// What the parenthesized head of a `for` turned out to be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Head {
    /// `init; test; update`
    Classic,
    /// `target in object`
    In,
    /// `target of iterable`
    Of,
}

impl<'a, C: Config> Skim<'_, 'a, C> {
    pub(super) fn for_statement(&mut self) -> Seen {
        self.bump();
        self.expect(Kind::LParen)?;
        self.scopes.enter(false);
        let no_in = std::mem::replace(&mut self.no_in, true);
        let head = self.for_head();
        self.no_in = no_in;
        match head? {
            Head::Classic => self.classic_rest()?,
            Head::In => {
                self.bump();
                self.expression()?;
            }
            Head::Of => {
                self.bump();
                self.assignment()?;
            }
        }
        self.expect(Kind::RParen)?;
        self.loop_body()?;
        self.scopes.exit();
        Some(())
    }

    fn for_head(&mut self) -> Seen<Head> {
        match self.kind() {
            Kind::Semicolon => Some(Head::Classic),
            Kind::Var => self.declared_head(DeclKind::Var),
            Kind::Const => self.declared_head(DeclKind::Const),
            Kind::Let => {
                let next = self.p.lexer.peek_token().kind();
                if !(matches!(next, Kind::LBrack | Kind::LCurly) || super::is_plain_identifier(next)) {
                    return None;
                }
                self.declared_head(DeclKind::Let)
            }
            _ => self.expression_head(),
        }
    }

    /// `var`, `let` or `const` declarations: looping over something takes one without
    /// an initializer.
    fn declared_head(&mut self, kind: DeclKind) -> Seen<Head> {
        self.bump();
        let declared = self.declarations(kind, true)?;
        match self.loop_keyword() {
            Some(head) if declared.count == 1 && !declared.init => Some(head),
            _ => (!declared.incomplete).then_some(Head::Classic),
        }
    }

    /// An expression, or the target of a loop over something: an assignment target or a
    /// pattern, whose shorthand initializers the loop keyword turns into defaults.
    fn expression_head(&mut self) -> Seen<Head> {
        let pending = self.pending_cover;
        let form = self.element_assignment()?;
        if let Some(head) = self.loop_keyword()
            && form.is_pattern_element()
            && form != Form::Default
            && !self.is_restricted_name(form)
        {
            self.pending_cover = pending;
            return Some(head);
        }
        if self.pending_cover != pending {
            return None;
        }
        while self.eat(Kind::Comma) {
            self.assignment()?;
        }
        Some(Head::Classic)
    }

    /// `in` or `of` at the current token.
    fn loop_keyword(&self) -> Option<Head> {
        match self.kind() {
            Kind::In => Some(Head::In),
            Kind::Of => Some(Head::Of),
            _ => None,
        }
    }

    /// `; test; update` of a classic `for`.
    fn classic_rest(&mut self) -> Seen {
        self.expect(Kind::Semicolon)?;
        if self.kind() != Kind::Semicolon {
            self.expression()?;
        }
        self.expect(Kind::Semicolon)?;
        if self.kind() != Kind::RParen {
            self.expression()?;
        }
        Some(())
    }
}
