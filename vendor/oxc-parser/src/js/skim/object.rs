//! Array and object literals of a skimmed body, with whether each could be a pattern.
use super::expr::Form;
use super::{Seen, Skim, is_plain_identifier, is_restricted};
use crate::{ParserConfig as Config, lexer::Kind};

impl<'a, C: Config> Skim<'_, 'a, C> {
    pub(super) fn array_literal(&mut self) -> Seen<Form> {
        self.bump();
        let mut pattern = true;
        while self.kind() != Kind::RBrack {
            if self.eat(Kind::Comma) {
                continue;
            }
            let spread = self.eat(Kind::Dot3);
            let form = self.nested(Self::element_assignment)?;
            pattern &= self.is_pattern_element(form) && !(spread && form == Form::Default);
            if self.kind() != Kind::RBrack {
                self.expect(Kind::Comma)?;
                pattern &= !spread;
            }
        }
        self.bump();
        Some(Form::Array { pattern })
    }

    /// `form`, just read, may be an element of a pattern.
    fn is_pattern_element(&self, form: Form) -> bool {
        form.is_pattern_element() && !self.is_restricted_name(form)
    }

    /// `form`, just read, is `eval` or `arguments`.
    pub(super) fn is_restricted_name(&self, form: Form) -> bool {
        form == Form::Ident && is_restricted(self.last_ident)
    }

    pub(super) fn object_literal(&mut self) -> Seen<Form> {
        self.bump();
        let mut pattern = true;
        let mut protos = 0;
        while self.kind() != Kind::RCurly {
            let property = self.nested(Self::object_property)?;
            pattern &= property.pattern;
            protos += u32::from(property.proto);
            if self.kind() != Kind::RCurly {
                self.expect(Kind::Comma)?;
                pattern &= !property.rest;
            }
        }
        self.bump();
        (protos <= 1).then_some(Form::Object { pattern })
    }

    /// A property name: an identifier name, a string, a number or a computed key.
    pub(super) fn property_key(&mut self) -> Seen {
        match self.kind() {
            Kind::Str => self.string_literal().map(|_| ()),
            Kind::LBrack => {
                self.bump();
                self.assignment()?;
                self.expect(Kind::RBrack)
            }
            kind if kind.is_number() => self.number_literal().map(|_| ()),
            kind if kind.is_identifier_name() => Some(self.bump()),
            _ => None,
        }
    }

    fn object_property(&mut self) -> Seen<Property> {
        if self.eat(Kind::Dot3) {
            let form = self.element_assignment()?;
            let target = form.is_simple_target() && !self.is_restricted_name(form);
            return Some(Property { pattern: target, rest: true, proto: false });
        }
        if self.eat(Kind::Star) {
            self.property_key()?;
            return self.method(false, true).map(|()| Property::method());
        }
        let token = self.p.cur_token();
        let name = self.p.cur_src();
        let plain = is_plain_identifier(token.kind()) && !token.escaped();
        if matches!(token.kind(), Kind::Get | Kind::Set) && self.accessor_follows() {
            self.accessor()?;
            return Some(Property::method());
        }
        self.property_key()?;
        match self.kind() {
            Kind::Colon => self.keyed_property(name, token.kind() == Kind::LBrack),
            Kind::LParen => self.method(false, false).map(|()| Property::method()),
            _ if plain => self.shorthand_property(name),
            _ => None,
        }
    }

    /// `get` or `set` is an accessor's keyword when a property name follows it.
    pub(super) fn accessor_follows(&mut self) -> bool {
        let next = self.p.lexer.peek_token().kind();
        !matches!(next, Kind::Colon | Kind::LParen | Kind::Comma | Kind::RCurly | Kind::Eq)
    }

    /// `key: value`: a computed key is never `__proto__`.
    fn keyed_property(&mut self, key: &str, computed: bool) -> Seen<Property> {
        self.bump();
        let form = self.element_assignment()?;
        let proto = !computed && matches!(key.trim_matches(['"', '\'']), "__proto__");
        Some(Property { pattern: self.is_pattern_element(form), rest: false, proto })
    }

    /// `name`, or `name = default` where a pattern may follow.
    fn shorthand_property(&mut self, name: &'a str) -> Seen<Property> {
        if is_restricted(name) {
            return None;
        }
        self.scopes.reference(name);
        if self.eat(Kind::Eq) {
            self.assignment()?;
            self.pending_cover += 1;
        }
        Some(Property { pattern: true, rest: false, proto: false })
    }
}

/// What a property tells of the literal it is in.
pub(super) struct Property {
    /// It may be part of a pattern.
    pattern: bool,
    /// It is a `...rest` element, which a pattern ends with.
    rest: bool,
    /// It is a `__proto__: value` property.
    proto: bool,
}

impl Property {
    fn method() -> Self {
        Self { pattern: false, rest: false, proto: false }
    }
}
