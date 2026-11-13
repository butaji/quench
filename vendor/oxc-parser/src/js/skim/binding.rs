//! Declarations and the binding patterns of a skimmed body.
use super::{Seen, Skim};
use crate::{ParserConfig as Config, lexer::Kind};

/// What a binding declares.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DeclKind {
    Var,
    Let,
    Const,
    Param,
}

/// What the declarators of a declaration came to.
pub(super) struct Declared {
    pub(super) count: usize,
    /// The last has an initializer.
    pub(super) init: bool,
    /// Some declarator lacks an initializer it needs.
    pub(super) incomplete: bool,
}

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// The declarators of a `var`, `let` or `const`. A `const` or a pattern needs an
    /// initializer, unless it is the only declarator in the head of a `for` that loops over
    /// something: the head says whether one lacks it.
    pub(super) fn declarations(&mut self, kind: DeclKind, for_head: bool) -> Seen<Declared> {
        let mut declared = Declared { count: 0, init: false, incomplete: false };
        loop {
            let pattern = self.binding_target(kind)?;
            declared.init = self.eat(Kind::Eq);
            if declared.init {
                self.assignment()?;
            } else if pattern || kind == DeclKind::Const {
                declared.incomplete = true;
            }
            declared.count += 1;
            if !self.eat(Kind::Comma) {
                return (for_head || !declared.incomplete).then_some(declared);
            }
        }
    }

    /// A binding identifier or pattern, declared as `kind`: whether it is a pattern.
    pub(super) fn binding_target(&mut self, kind: DeclKind) -> Seen<bool> {
        match self.kind() {
            Kind::LBrack => self.deeper(|this| this.array_binding(kind))?,
            Kind::LCurly => self.deeper(|this| this.object_binding(kind))?,
            _ => {
                let name = self.identifier()?;
                if super::is_restricted(name) {
                    return None;
                }
                self.declare(kind, name)?;
                return Some(false);
            }
        }
        Some(true)
    }

    pub(super) fn declare(&mut self, kind: DeclKind, name: &'a str) -> Seen {
        match kind {
            DeclKind::Var => self.scopes.declare_var(name),
            DeclKind::Let | DeclKind::Const => self.scopes.declare_lexical(name),
            DeclKind::Param => self.scopes.declare_param(name),
        }
    }

    /// A binding with its default: whether it is a plain identifier without one.
    pub(super) fn binding_element(&mut self, kind: DeclKind) -> Seen<bool> {
        let pattern = self.binding_target(kind)?;
        let default = self.eat(Kind::Eq);
        if default {
            self.assignment()?;
        }
        Some(!pattern && !default)
    }

    fn array_binding(&mut self, kind: DeclKind) -> Seen {
        self.bump();
        while self.kind() != Kind::RBrack {
            if self.eat(Kind::Comma) {
                continue;
            }
            if self.eat(Kind::Dot3) {
                self.binding_target(kind)?;
                break;
            }
            self.binding_element(kind)?;
            if self.kind() != Kind::RBrack {
                self.expect(Kind::Comma)?;
            }
        }
        self.expect(Kind::RBrack)
    }

    fn object_binding(&mut self, kind: DeclKind) -> Seen {
        self.bump();
        while self.kind() != Kind::RCurly {
            if self.eat(Kind::Dot3) {
                if self.binding_target(kind)? {
                    return None;
                }
                break;
            }
            self.binding_property(kind)?;
            if self.kind() != Kind::RCurly {
                self.expect(Kind::Comma)?;
            }
        }
        self.expect(Kind::RCurly)
    }

    /// `key: element`, or a shorthand `name` with its default.
    fn binding_property(&mut self, kind: DeclKind) -> Seen {
        let token = self.p.cur_token();
        let shorthand = (super::is_plain_identifier(token.kind()) && !token.escaped()).then(|| self.p.cur_src());
        self.property_key()?;
        if self.eat(Kind::Colon) {
            return self.binding_element(kind).map(|_| ());
        }
        let name = shorthand?;
        if super::is_restricted(name) {
            return None;
        }
        self.declare(kind, name)?;
        if self.eat(Kind::Eq) {
            self.assignment()?;
        }
        Some(())
    }
}
