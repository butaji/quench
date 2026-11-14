//! A syntax-only pass over the body of a lazily parsed function: one scan of the tokens, no
//! tree. It accepts exactly the bodies whose syntax and early errors it can decide alone
//! (`Seen::None` anywhere means "not decided": the caller parses the body whole, as before),
//! and leaves the names the body reads from outside itself, which is what a stub of the
//! function needs. Declarations and references are tracked as in a pre-parser: each scope
//! knows its names, each reference is settled when its scope closes.
//!
//! Not decided here, so always parsed whole: private names, `with`, `import`/`export`, class
//! static blocks, `let`/`yield`/`await`/`static`... as identifiers outside their own contexts,
//! a `"use strict"` prologue with a non-simple parameter list, duplicate declarations in one
//! scope that a sloppy program may repeat, and legacy octal literals and escapes.
use crate::{ParserConfig as Config, ParserImpl, lexer::Kind};
use oxc_ast::ast::FormalParameters;
use oxc_span::GetSpan;

mod binding;
mod class;
mod expr;
mod flow;
mod for_loop;
mod function;
mod object;
mod primary;
mod scope;
mod stmt;

use flow::Flow;
use scope::Scopes;

/// `Some` when the construct was decided valid, `None` when it was not decided.
type Seen<T = ()> = Option<T>;

/// Nesting of statements and expressions beyond which a body is not skimmed: the whole parse
/// reports stack exhaustion with its own accounting.
const MAX_DEPTH: u32 = 160;

/// The names a function reads from outside itself.
pub(crate) struct Skimmed<'a> {
    pub(crate) free_names: Vec<&'a str>,
    /// The parameters are plain names, so no default shares the function's scope.
    pub(crate) names_exact: bool,
    /// The directives of the skimmed function's own body, as the start of each, the end of its
    /// string and the end of its statement; none has an escape.
    pub(crate) directives: Vec<(u32, u32, u32)>,
}

struct Skim<'p, 'a, C: Config> {
    p: &'p mut ParserImpl<'a, C>,
    scopes: Scopes<'a>,
    flow: Flow<'a>,
    strict: bool,
    depth: u32,
    /// Shorthand properties with an initializer (`{a = 1}`) read so far that no assignment
    /// has yet turned into a pattern.
    pending_cover: u32,
    /// The assignment expression about to parse is an element of an array or object literal,
    /// which an enclosing assignment may turn into a pattern.
    in_cover: bool,
    /// `in` is not an operator here: the head of a `for`.
    no_in: bool,
    /// The identifier last read as a reference or a label.
    last_ident: &'a str,
    ctx: FnCtx,
    /// The directives of the first function body read, which is the skimmed function's own.
    directives: Vec<(u32, u32, u32)>,
    directives_read: bool,
}

/// Whether a function is a generator, and whether it is `async`.
#[derive(Clone, Copy, Default)]
struct Flavor {
    generator: bool,
    is_async: bool,
}

/// What kind of function a skim starts from: the code around it decides what it may use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LazyForm {
    Plain,
    /// An arrow function, which uses the `this`, `arguments`, `new.target` and `super` of the code
    /// around it.
    Arrow,
}

impl LazyForm {
    fn ctx(self) -> FnCtx {
        match self {
            Self::Plain => FnCtx::PLAIN,
            Self::Arrow => FnCtx::ARROW,
        }
    }
}

/// What the code of a function may use that the function around it decides.
#[derive(Clone, Copy)]
struct FnCtx {
    /// `super.x` is valid: in a method, or an arrow inside one.
    super_property: bool,
    /// `super(...)` is valid: in the constructor of a derived class, or an arrow inside it.
    super_call: bool,
    /// `yield` is an operator: in a generator.
    generator: bool,
    /// `await` is an operator: in an `async` function.
    is_async: bool,
    /// In the initializer of a class field, or an arrow inside one: `arguments` is not valid.
    field_init: bool,
    /// In the skimmed arrow, or an arrow inside it, outside every function of its own: `this`,
    /// `arguments`, `new.target` and `super` belong to the code around the arrow, which the
    /// skim does not see, so none is decided.
    lexical: bool,
}

impl FnCtx {
    const PLAIN: Self = Self { super_property: false, super_call: false, generator: false, is_async: false, field_init: false, lexical: false };
    const ARROW: Self = Self { lexical: true, ..Self::PLAIN };

    fn function(flavor: Flavor) -> Self {
        Self { generator: flavor.generator, is_async: flavor.is_async, ..Self::PLAIN }
    }

    fn method(super_call: bool, flavor: Flavor) -> Self {
        Self { super_property: true, super_call, ..Self::function(flavor) }
    }

    fn field() -> Self {
        Self { super_property: true, field_init: true, ..Self::PLAIN }
    }
}

impl<'a, C: Config> ParserImpl<'a, C> {
    /// The names the function at the current `{` reads from outside itself, when its body, and
    /// its parameters where they are not plain names, are decided valid; the parser then
    /// stands after the closing `}`. The parameters of such a function are read again from
    /// `head`, where the parser stood at their `(`. Otherwise the parser may stand anywhere:
    /// the caller rewinds.
    pub(crate) fn skim_function_body(&mut self, params: &FormalParameters<'a>, form: LazyForm, head: Option<crate::cursor::ParserCheckpoint<'a>>) -> Option<Skimmed<'a>> {
        let names = self.simple_param_names(params);
        if names.is_none() {
            let head = head.filter(|head| self.recorded_nothing_since(head))?;
            self.rewind(head);
        }
        let strict = self.strict;
        let mut skim = Skim {
            p: self,
            scopes: Scopes::default(),
            flow: Flow::default(),
            strict,
            depth: 0,
            pending_cover: 0,
            in_cover: false,
            no_in: false,
            last_ident: "",
            ctx: form.ctx(),
            directives: Vec::new(),
            directives_read: false,
        };
        skim.scopes.enter(true);
        let names_exact = names.is_some();
        match names {
            Some(names) => {
                for name in names {
                    skim.scopes.declare_param(name)?;
                }
                skim.function_body(true)?;
            }
            None => skim.function_from_head(form == LazyForm::Arrow)?,
        }
        skim.scopes.exit();
        Some(Skimmed { free_names: std::mem::take(&mut skim.scopes.refs), names_exact, directives: skim.directives })
    }

    /// The names of `params` when each is a plain identifier: no pattern, default, rest or
    /// repeat, and none `eval` or `arguments`.
    fn simple_param_names(&self, params: &FormalParameters<'a>) -> Option<Vec<&'a str>> {
        if params.rest.is_some() {
            return None;
        }
        let mut names = Vec::with_capacity(params.items.len());
        for param in &params.items {
            let oxc_ast::ast::BindingPattern::BindingIdentifier(identifier) = &param.pattern else { return None };
            let span = identifier.span();
            let name = self.source_text.get(span.start as usize..span.end as usize)?;
            if param.initializer.is_some() || name.len() != identifier.name.len() || names.contains(&name) || is_restricted(name) {
                return None;
            }
            names.push(name);
        }
        Some(names)
    }
}

/// `eval` and `arguments` cannot be bound or assigned in strict code.
fn is_restricted(name: &str) -> bool {
    matches!(name, "eval" | "arguments")
}

/// Whether `kind` is an identifier that is valid as a name in any code: not reserved, not
/// reserved in strict code.
fn is_plain_identifier(kind: Kind) -> bool {
    kind.is_identifier() && !kind.is_strict_mode_contextual_keyword()
}

impl<'a, C: Config> Skim<'_, 'a, C> {
    fn kind(&self) -> Kind {
        self.p.cur_kind()
    }

    fn bump(&mut self) {
        self.p.bump_any();
    }

    fn eat(&mut self, kind: Kind) -> bool {
        let at = self.kind() == kind;
        if at {
            self.bump();
        }
        at
    }

    fn expect(&mut self, kind: Kind) -> Seen {
        self.eat(kind).then_some(())
    }

    /// The current token has a line break before it.
    fn newline(&self) -> bool {
        self.p.cur_token().is_on_new_line()
    }

    /// Runs `f` one nesting level deeper.
    fn deeper<T>(&mut self, f: impl FnOnce(&mut Self) -> Seen<T>) -> Seen<T> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let seen = f(self);
        self.depth -= 1;
        seen
    }

    /// The end of a statement: `;`, or the automatic one before `}`, the end of the text or a
    /// line break.
    fn semicolon(&mut self) -> Seen {
        (self.eat(Kind::Semicolon) || matches!(self.kind(), Kind::RCurly | Kind::Eof) || self.newline()).then_some(())
    }

    /// The current token as a plain identifier (see `is_plain_identifier`), read.
    /// Runs `f` in function context `ctx`.
    fn with_ctx<T>(&mut self, ctx: FnCtx, f: impl FnOnce(&mut Self) -> Seen<T>) -> Seen<T> {
        let saved = std::mem::replace(&mut self.ctx, ctx);
        let seen = f(self);
        self.ctx = saved;
        seen
    }

    fn identifier(&mut self) -> Seen<&'a str> {
        let token = self.p.cur_token();
        if !is_plain_identifier(token.kind()) || token.escaped() {
            return None;
        }
        let name = self.p.cur_src();
        self.bump();
        Some(name)
    }
}
