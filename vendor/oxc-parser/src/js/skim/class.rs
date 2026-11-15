//! Classes of a skimmed body: methods, accessors and static members; not fields, static
//! blocks, private names, generators or `async` methods.
use super::expr::Form;
use super::{FnCtx, Seen, Skim, is_plain_identifier, is_restricted};
use crate::{ParserConfig as Config, lexer::Kind};

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// `class Name ... { ... }`, its name declared where it stands.
    pub(super) fn class_declaration(&mut self) -> Seen {
        self.bump();
        let name = self.identifier()?;
        if is_restricted(name) {
            return None;
        }
        self.scopes.declare_lexical(name)?;
        self.class_tail()
    }

    /// `class Name? ... { ... }`, its name bound inside itself.
    pub(super) fn class_expression(&mut self) -> Seen<Form> {
        self.bump();
        self.scopes.enter(false);
        if is_plain_identifier(self.kind()) {
            let name = self.identifier()?;
            self.scopes.declare_name(name);
        }
        self.class_tail()?;
        self.scopes.exit();
        Some(Form::Other)
    }

    /// The heritage and the body: strict code.
    fn class_tail(&mut self) -> Seen {
        let strict = std::mem::replace(&mut self.strict, true);
        let derived = self.eat(Kind::Extends);
        let seen = self.class_heritage(derived).and_then(|()| self.class_body(derived));
        self.strict = strict;
        seen
    }

    fn class_heritage(&mut self, derived: bool) -> Seen {
        if derived { self.deeper(Self::lhs).map(|_| ()) } else { Some(()) }
    }

    fn class_body(&mut self, derived: bool) -> Seen {
        self.expect(Kind::LCurly)?;
        let mut constructed = false;
        while self.kind() != Kind::RCurly {
            if !self.eat(Kind::Semicolon) {
                self.class_member(derived, &mut constructed)?;
            }
        }
        self.bump();
        Some(())
    }

    /// `static` is a modifier when a member follows it.
    fn static_modifier(&mut self) -> bool {
        let is_static = self.kind() == Kind::Static
            && !matches!(self.p.lexer.peek_token().kind(), Kind::LParen | Kind::Eq | Kind::Semicolon | Kind::RCurly | Kind::LCurly);
        if is_static {
            self.bump();
        }
        is_static
    }

    /// A method, an accessor or a field; `constructed` says whether a constructor was read.
    fn class_member(&mut self, derived: bool, constructed: &mut bool) -> Seen {
        let is_static = self.static_modifier();
        let prefix = self.member_prefix();
        let token = self.p.cur_token();
        let key = self.p.cur_src();
        if token.kind() == Kind::Str && key.contains('\\') {
            return None;
        }
        let special = self.special_key(token.kind(), key);
        self.property_key()?;
        if is_static && special == Some("prototype") {
            return None;
        }
        let constructor = special == Some("constructor") && !is_static;
        if constructor && (prefix.accessor.is_some() || prefix.modified() || std::mem::replace(constructed, true)) {
            return None;
        }
        if let Some(setter) = prefix.accessor {
            return self.accessor_rest(setter);
        }
        if self.kind() == Kind::LParen {
            return self.method(constructor && derived, prefix.flavor);
        }
        (!prefix.modified() && special != Some("constructor")).then_some(())?;
        self.field()
    }

    /// A field's initializer is code of a method of its own, and the field ends its line.
    fn field(&mut self) -> Seen {
        if self.eat(Kind::Eq) {
            self.with_ctx(FnCtx::field(), |this| this.nested(Self::assignment).map(|_| ()))?;
        }
        self.semicolon()
    }

    /// The names a class member's key may not take: `constructor` and `prototype`, written
    /// plainly.
    fn special_key(&self, kind: Kind, key: &'a str) -> Option<&'a str> {
        let name = match kind {
            Kind::Str => key.get(1..key.len() - 1).filter(|inner| !inner.contains('\\'))?,
            kind if kind.is_identifier_name() => key,
            _ => return None,
        };
        matches!(name, "constructor" | "prototype").then_some(name)
    }
}
