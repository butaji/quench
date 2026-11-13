//! Statements of a skimmed body.
use super::binding::DeclKind;
use super::expr::Form;
use super::{Seen, Skim};
use crate::{ParserConfig as Config, lexer::Kind};

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// Statements up to the closing `}` of the block or body, which is left unread.
    pub(super) fn statements(&mut self) -> Seen {
        while self.kind() != Kind::RCurly {
            self.statement_item()?;
        }
        Some(())
    }

    /// A statement or a declaration.
    fn statement_item(&mut self) -> Seen {
        match self.kind() {
            Kind::Function => self.deeper(Self::function_declaration),
            Kind::Class => self.deeper(Self::class_declaration),
            Kind::Const => self.deeper(|this| this.declaration_statement(DeclKind::Const)),
            Kind::Let => self.let_declaration(),
            _ => self.statement(),
        }
    }

    /// `let` starting a declaration: it names a binding next, as a plain `let` identifier is
    /// not skimmed.
    fn let_declaration(&mut self) -> Seen {
        let next = self.p.lexer.peek_token().kind();
        if !(matches!(next, Kind::LBrack | Kind::LCurly) || super::is_plain_identifier(next)) {
            return None;
        }
        self.deeper(|this| this.declaration_statement(DeclKind::Let))
    }

    fn declaration_statement(&mut self, kind: DeclKind) -> Seen {
        self.bump();
        self.declarations(kind, false)?;
        self.semicolon()
    }

    pub(super) fn statement(&mut self) -> Seen {
        let run = self.take_label_run();
        self.deeper(|this| this.statement_of(run))
    }

    fn statement_of(&mut self, run: usize) -> Seen {
        match self.kind() {
            Kind::LCurly => self.block(),
            Kind::Var => self.declaration_statement(DeclKind::Var),
            Kind::Semicolon => {
                self.bump();
                Some(())
            }
            Kind::Debugger => {
                self.bump();
                self.semicolon()
            }
            Kind::If => self.if_statement(),
            Kind::For | Kind::While | Kind::Do => self.loop_statement(run),
            Kind::Return | Kind::Throw => self.exit_statement(),
            Kind::Break | Kind::Continue => self.jump(self.kind()),
            Kind::Try => self.try_statement(),
            Kind::Switch => self.switch_statement(),
            Kind::Function | Kind::Class | Kind::Let | Kind::Import | Kind::Export | Kind::With => None,
            _ => self.expression_statement(run),
        }
    }

    fn expression_statement(&mut self, run: usize) -> Seen {
        let form = self.expression()?;
        if form == Form::Ident && self.kind() == Kind::Colon {
            let name = self.last_ident;
            self.scopes.refs.pop();
            self.bump();
            return self.labelled(name, run);
        }
        self.semicolon()
    }

    /// `{ ... }`, a scope of its own.
    fn block(&mut self) -> Seen {
        self.expect(Kind::LCurly)?;
        self.scopes.enter(false);
        self.statements()?;
        self.scopes.exit();
        self.expect(Kind::RCurly)
    }

    fn condition(&mut self) -> Seen {
        self.expect(Kind::LParen)?;
        self.expression()?;
        self.expect(Kind::RParen)
    }

    fn if_statement(&mut self) -> Seen {
        self.bump();
        self.condition()?;
        self.statement()?;
        if self.eat(Kind::Else) {
            self.statement()?;
        }
        Some(())
    }

    /// `return` and `throw`: the operand of `throw` starts on its line.
    fn exit_statement(&mut self) -> Seen {
        let throws = self.kind() == Kind::Throw;
        self.bump();
        let operand = !(self.newline() || matches!(self.kind(), Kind::Semicolon | Kind::RCurly | Kind::Eof));
        if operand {
            self.expression()?;
        } else if throws {
            return None;
        }
        self.semicolon()
    }

    /// The body of a loop, which `break` and `continue` may leave.
    pub(super) fn loop_body(&mut self) -> Seen {
        self.flow.loops += 1;
        let seen = self.statement();
        self.flow.loops -= 1;
        seen
    }

    fn loop_statement(&mut self, run: usize) -> Seen {
        if run > 0 {
            self.mark_loop_labels(run);
        }
        match self.kind() {
            Kind::While => {
                self.bump();
                self.condition()?;
                self.loop_body()
            }
            Kind::Do => {
                self.bump();
                self.loop_body()?;
                self.expect(Kind::While)?;
                self.condition()?;
                self.eat(Kind::Semicolon);
                Some(())
            }
            _ => self.for_statement(),
        }
    }

    fn try_statement(&mut self) -> Seen {
        self.bump();
        self.block()?;
        let caught = self.kind() == Kind::Catch;
        if caught {
            self.catch_clause()?;
        }
        if self.eat(Kind::Finally) {
            self.block()?;
        } else if !caught {
            return None;
        }
        Some(())
    }

    /// `catch (name) { ... }` or `catch { ... }`: the parameter and the block share a scope.
    fn catch_clause(&mut self) -> Seen {
        self.bump();
        self.scopes.enter(false);
        if self.eat(Kind::LParen) {
            let name = self.identifier()?;
            self.scopes.declare_catch(name)?;
            self.expect(Kind::RParen)?;
        }
        self.expect(Kind::LCurly)?;
        self.statements()?;
        self.scopes.exit();
        self.expect(Kind::RCurly)
    }

    fn switch_statement(&mut self) -> Seen {
        self.bump();
        self.condition()?;
        self.expect(Kind::LCurly)?;
        self.scopes.enter(false);
        self.flow.switches += 1;
        let seen = self.cases();
        self.flow.switches -= 1;
        seen?;
        self.scopes.exit();
        self.expect(Kind::RCurly)
    }

    /// The clauses of a `switch`, at most one of them `default`.
    fn cases(&mut self) -> Seen {
        let mut has_default = false;
        while self.kind() != Kind::RCurly {
            if self.eat(Kind::Default) {
                if std::mem::replace(&mut has_default, true) {
                    return None;
                }
            } else {
                self.expect(Kind::Case)?;
                self.expression()?;
            }
            self.expect(Kind::Colon)?;
            while !matches!(self.kind(), Kind::Case | Kind::Default | Kind::RCurly) {
                self.statement_item()?;
            }
        }
        Some(())
    }
}
