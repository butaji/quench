//! Expressions of a skimmed body, by precedence. Each returns the `Form` it read: just enough
//! of its shape for the rules that look at an operand (assignment targets, `**`, `??`).
use super::{Seen, Skim};
use crate::{ParserConfig as Config, lexer::Kind};

/// The syntactic shape of an expression, as far as its context needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Form {
    Ident,
    Member,
    Call,
    /// An optional chain: not an assignment target.
    Chain,
    /// `[...]`, which an assignment may read as a pattern when `pattern`.
    Array { pattern: bool },
    Object { pattern: bool },
    /// `target = value`, which a pattern holds as an element with a default.
    Default,
    /// `(target)`: still an assignment target.
    ParenTarget,
    Paren,
    Unary,
    /// `||` or `&&`.
    Logical,
    Coalesce,
    Arrow,
    Other,
}

impl Form {
    /// An assignment target of its own: a name or a member, however parenthesized.
    pub(super) fn is_simple_target(self) -> bool {
        matches!(self, Self::Ident | Self::Member | Self::ParenTarget)
    }

    /// An element of a pattern.
    pub(super) fn is_pattern_element(self) -> bool {
        self.is_simple_target() || matches!(self, Self::Default | Self::Array { pattern: true } | Self::Object { pattern: true })
    }
}

/// The binding strength of a binary operator; `in` is none inside the head of a `for`.
fn precedence(kind: Kind, no_in: bool) -> Option<u8> {
    Some(match kind {
        Kind::Question2 => 1,
        Kind::Pipe2 => 2,
        Kind::Amp2 => 3,
        Kind::Pipe => 4,
        Kind::Caret => 5,
        Kind::Amp => 6,
        Kind::Eq2 | Kind::Neq | Kind::Eq3 | Kind::Neq2 => 7,
        Kind::LAngle | Kind::RAngle | Kind::LtEq | Kind::GtEq | Kind::Instanceof => 8,
        Kind::In if !no_in => 8,
        Kind::ShiftLeft | Kind::ShiftRight | Kind::ShiftRight3 => 9,
        Kind::Plus | Kind::Minus => 10,
        Kind::Star | Kind::Slash | Kind::Percent => 11,
        Kind::Star2 => 12,
        _ => return None,
    })
}

/// The form of `left op right`: `**` takes no unary operand on its left, and `??` mixes with
/// `||` and `&&` only through parentheses.
fn combine(op: Kind, left: Form, right: Form) -> Seen<Form> {
    let operands = [left, right];
    match op {
        Kind::Star2 if left == Form::Unary => None,
        Kind::Pipe2 | Kind::Amp2 if operands.contains(&Form::Coalesce) => None,
        Kind::Question2 if operands.contains(&Form::Logical) => None,
        Kind::Pipe2 | Kind::Amp2 => Some(Form::Logical),
        Kind::Question2 => Some(Form::Coalesce),
        _ => Some(Form::Other),
    }
}

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// Expression, with commas.
    pub(super) fn expression(&mut self) -> Seen<Form> {
        let mut form = self.assignment()?;
        while self.eat(Kind::Comma) {
            self.assignment()?;
            form = Form::Other;
        }
        Some(form)
    }

    /// An element of an array or object literal, which an assignment around it may turn
    /// into a pattern.
    pub(super) fn element_assignment(&mut self) -> Seen<Form> {
        self.in_cover = true;
        self.assignment()
    }

    pub(super) fn assignment(&mut self) -> Seen<Form> {
        self.deeper(Self::assignment_of)
    }

    fn assignment_of(&mut self) -> Seen<Form> {
        let in_cover = std::mem::take(&mut self.in_cover);
        let pending = self.pending_cover;
        if self.kind() == Kind::LParen && self.try_arrow()? {
            return Some(Form::Arrow);
        }
        if self.kind() == Kind::Yield && self.ctx.generator {
            return self.yield_expression();
        }
        let mut form = self.conditional()?;
        if form == Form::Ident && self.kind() == Kind::Arrow && !self.newline() {
            return self.identifier_arrow();
        }
        if self.kind().is_assignment_operator() {
            form = self.assign(form, pending)?;
        }
        (in_cover || self.pending_cover == pending).then_some(form)
    }

    /// `yield`, `yield value` or `yield* iterable`: nothing follows a bare one on its line.
    fn yield_expression(&mut self) -> Seen<Form> {
        self.bump();
        let bare = self.newline()
            || matches!(self.kind(), Kind::RParen | Kind::RBrack | Kind::RCurly | Kind::Comma | Kind::Semicolon | Kind::Colon | Kind::Eof | Kind::In);
        if !bare {
            self.eat(Kind::Star);
            self.assignment()?;
        }
        Some(Form::Other)
    }

    /// `form op value`, `form` being the target already read.
    fn assign(&mut self, target: Form, pending: u32) -> Seen<Form> {
        let eq = self.kind() == Kind::Eq;
        if self.is_restricted_name(target) {
            return None;
        }
        let pattern = matches!(target, Form::Array { pattern: true } | Form::Object { pattern: true });
        if !(target.is_simple_target() || eq && pattern) {
            return None;
        }
        if pattern {
            self.pending_cover = pending;
        }
        self.bump();
        self.assignment()?;
        Some(if eq { Form::Default } else { Form::Other })
    }

    fn conditional(&mut self) -> Seen<Form> {
        let test = self.binary(1)?;
        if !self.eat(Kind::Question) {
            return Some(test);
        }
        let no_in = std::mem::replace(&mut self.no_in, false);
        self.assignment()?;
        self.no_in = no_in;
        self.expect(Kind::Colon)?;
        self.assignment()?;
        Some(Form::Other)
    }

    fn binary(&mut self, min: u8) -> Seen<Form> {
        let mut left = self.unary()?;
        // `>` may start `>=`, `>>`, `>>>` or an assignment: the lexer leaves it to the parser.
        while let Some(strength) = precedence(self.p.re_lex_right_angle(), self.no_in).filter(|strength| *strength >= min) {
            let op = self.kind();
            self.bump();
            let right = self.binary(if op == Kind::Star2 { strength } else { strength + 1 })?;
            left = combine(op, left, right)?;
        }
        Some(left)
    }

    fn unary(&mut self) -> Seen<Form> {
        match self.kind() {
            Kind::Bang | Kind::Tilde | Kind::Plus | Kind::Minus | Kind::Typeof | Kind::Void => {
                self.bump();
                self.deeper(Self::unary)?;
                Some(Form::Unary)
            }
            Kind::Delete => self.delete(),
            Kind::Plus2 | Kind::Minus2 => {
                self.bump();
                let operand = self.deeper(Self::unary)?;
                self.update_target(operand)?;
                Some(Form::Other)
            }
            _ => self.postfix(),
        }
    }

    /// `delete operand`: not a name in strict code.
    fn delete(&mut self) -> Seen<Form> {
        self.bump();
        let operand = self.deeper(Self::unary)?;
        if self.strict && matches!(operand, Form::Ident | Form::ParenTarget) {
            return None;
        }
        Some(Form::Unary)
    }

    fn postfix(&mut self) -> Seen<Form> {
        let form = self.lhs()?;
        if matches!(self.kind(), Kind::Plus2 | Kind::Minus2) && !self.newline() {
            self.update_target(form)?;
            self.bump();
            return Some(Form::Other);
        }
        Some(form)
    }

    /// `operand` of `++` or `--` is a target, and not `eval` or `arguments`.
    fn update_target(&self, operand: Form) -> Seen {
        (operand.is_simple_target() && !self.is_restricted_name(operand)).then_some(())
    }
}
