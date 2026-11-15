//! Scopes of a lazily parsed function tracked during its parse (Quench), so its check needs no
//! second walk over its tree: the names its subtree reads from outside it, and whether a
//! semantic early error may lie in it. Active only while the parser is inside a function whose
//! body `ParseOptions::lazy_function_bodies` releases (`ParserImpl::early`).
//!
//! The tracker is conservative: a construct an early-error rule of the semantic check could
//! fire on, and that the tracker does not model, makes the body it lies in `unsure`, and the
//! consumer then runs the full semantic check for that body as before. A body released inside
//! another is a frame of its own: its verdict does not spread to the body around it, while its
//! references do, as the check of the body around composes them. Its own state rewinds with the
//! parser (`EarlyCheckpoint`), so a speculative parse leaves nothing behind.

/// What a declared name is, as far as redeclaration and hoisting need.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DeclKind {
    /// `var`, or a function declaration at a function's top level.
    Var,
    /// `let`, `const`, `class`, or a function declaration in a block.
    Lexical,
    /// A `var` declared in an inner block, recorded on each scope it is hoisted through so a
    /// lexical declaration of that scope conflicts with it; binds nothing there.
    VarThrough,
    /// A catch clause's parameter (a plain name).
    CatchParam,
    /// A formal parameter.
    Param,
    /// A function expression's or a class's own name in its own scope: binds, never conflicts.
    OwnName,
}

/// What kind of scope, as far as hoisting needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ScopeKind {
    /// A function (its parameters and body), an arrow or a static block: `var` stops here.
    Function,
    /// A block, a `for` head, a `switch` body, a class or a catch clause.
    Block,
}

/// Where `super` and `arguments` may appear, per function boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Home {
    /// A plain function: no `super`.
    Plain,
    /// A method or accessor: `super.x`.
    Method,
    /// A base class's constructor: `super.x`.
    BaseConstructor,
    /// A derived class's constructor: `super.x` and `super()`.
    DerivedConstructor,
    /// A class field's initializer: `super.x`, no `arguments`.
    Field,
    /// A class static block: `super.x`, no `arguments`, no `await`.
    StaticBlock,
}

#[derive(Clone, Copy)]
struct Scope {
    kind: ScopeKind,
    decls_start: u32,
    refs_start: u32,
}

/// One function boundary: what `break`, `continue`, `super` and parameters see.
#[derive(Clone, Copy)]
struct Boundary {
    /// Its first label in `Early::labels`.
    labels_start: u32,
    home: Home,
    loops: u32,
    switches: u32,
    in_params: bool,
    simple_params: bool,
}

/// One lazily parsed body being tracked.
#[derive(Clone, Copy)]
struct Body {
    /// Its function's scope.
    scope: u32,
    /// The verdict of the body around it, restored when it ends.
    outer_unsure: bool,
}

#[derive(Default)]
pub(crate) struct Early<'a> {
    scopes: Vec<Scope>,
    decls: Vec<(&'a str, DeclKind)>,
    refs: Vec<&'a str>,
    boundaries: Vec<Boundary>,
    bodies: Vec<Body>,
    /// Whether each class being parsed has a heritage, and whether a constructor was seen in
    /// it, innermost last.
    classes: Vec<(bool, bool)>,
    /// The innermost body may hold an early error the tracker does not model.
    unsure: bool,
    /// `__proto__: value` properties seen (`parse_object_expression` counts them per object).
    pub(crate) protos: u32,
    /// The labels around, innermost last, each with whether it labels a loop.
    labels: Vec<(&'a str, bool)>,
    /// The references tracked are no longer exact (an escaped name): every body is unsure.
    lost: bool,
}

/// `Early`'s state at a parser checkpoint.
#[derive(Clone, Copy)]
pub(crate) struct EarlyCheckpoint {
    scopes: u32,
    decls: u32,
    refs: u32,
    boundaries: u32,
    bodies: u32,
    classes: u32,
    labels: u32,
    unsure: bool,
    lost: bool,
}

/// What the tracker found for one lazily parsed body.
pub(crate) struct EarlyVerdict<'a> {
    /// The names its subtree reads from outside it, sorted, without duplicates.
    pub(crate) free: Vec<&'a str>,
    /// Some early-error rule it does not model may fire in it.
    pub(crate) unsure: bool,
}

/// Names a semantic rule reads wherever they appear as an identifier (strict mode reserved
/// words, `await` in a static block): the tracker does not model those rules.
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "await" | "implements" | "interface" | "let" | "package" | "private" | "protected" | "public" | "static" | "yield"
    )
}

impl<'a> Early<'a> {
    /// Whether a function is being tracked.
    #[inline]
    pub(crate) fn active(&self) -> bool {
        !self.bodies.is_empty()
    }

    pub(crate) fn checkpoint(&self) -> EarlyCheckpoint {
        EarlyCheckpoint {
            scopes: self.scopes.len() as u32,
            decls: self.decls.len() as u32,
            refs: self.refs.len() as u32,
            boundaries: self.boundaries.len() as u32,
            bodies: self.bodies.len() as u32,
            classes: self.classes.len() as u32,
            labels: self.labels.len() as u32,
            unsure: self.unsure,
            lost: self.lost,
        }
    }

    pub(crate) fn rewind(&mut self, checkpoint: EarlyCheckpoint) {
        self.scopes.truncate(checkpoint.scopes as usize);
        self.decls.truncate(checkpoint.decls as usize);
        self.refs.truncate(checkpoint.refs as usize);
        self.boundaries.truncate(checkpoint.boundaries as usize);
        self.bodies.truncate(checkpoint.bodies as usize);
        self.classes.truncate(checkpoint.classes as usize);
        self.labels.truncate(checkpoint.labels as usize);
        self.unsure = checkpoint.unsure;
        self.lost = checkpoint.lost;
    }

    /// Starts tracking a lazily parsed function before its parameters, inside the body around
    /// it if that is tracked: its scope and boundary are entered.
    pub(crate) fn begin_body(&mut self) {
        if !self.active() {
            self.scopes.clear();
            self.decls.clear();
            self.refs.clear();
            self.boundaries.clear();
            self.classes.clear();
            self.labels.clear();
            self.unsure = false;
            self.lost = false;
        }
        self.bodies.push(Body { scope: self.scopes.len() as u32, outer_unsure: self.unsure });
        self.unsure = false;
        self.scopes.push(Scope { kind: ScopeKind::Function, decls_start: self.decls.len() as u32, refs_start: self.refs.len() as u32 });
        self.boundaries.push(Boundary { labels_start: self.labels.len() as u32, home: Home::Plain, loops: 0, switches: 0, in_params: false, simple_params: true });
    }

    /// Ends the innermost tracked body after its closing brace: its scope and boundary close,
    /// its free names pass on to the body around it, and its verdict returns.
    pub(crate) fn end_body(&mut self) -> Option<EarlyVerdict<'a>> {
        let body = self.bodies.last().copied()?;
        // A scope left open inside it (a parse that stopped early) closes with it.
        while self.scopes.len() > body.scope as usize + 1 {
            self.exit();
        }
        self.pop_boundary();
        let refs_start = self.scopes.last().map_or(0, |scope| scope.refs_start as usize);
        self.resolve_innermost();
        let mut free: Vec<&'a str> = self.refs[refs_start..].iter().copied().filter(|name| *name != "arguments").collect();
        free.sort_unstable();
        free.dedup();
        self.close_innermost();
        self.bodies.pop();
        let unsure = self.unsure || self.lost;
        self.unsure = body.outer_unsure;
        Some(EarlyVerdict { free, unsure })
    }

    #[inline]
    pub(crate) fn enter(&mut self, kind: ScopeKind) {
        if self.active() {
            self.scopes.push(Scope { kind, decls_start: self.decls.len() as u32, refs_start: self.refs.len() as u32 });
        }
    }

    /// Enters a function boundary and its scope: `home` None for an arrow, which keeps the
    /// home around it.
    pub(crate) fn enter_function(&mut self, home: Option<Home>) {
        if !self.active() {
            return;
        }
        let home = home.unwrap_or_else(|| self.boundaries.last().map_or(Home::Plain, |boundary| boundary.home));
        self.boundaries.push(Boundary { labels_start: self.labels.len() as u32, home, loops: 0, switches: 0, in_params: false, simple_params: true });
        self.enter(ScopeKind::Function);
    }

    pub(crate) fn exit_function(&mut self) {
        if self.active() {
            self.pop_boundary();
            self.exit();
        }
    }

    fn pop_boundary(&mut self) {
        if let Some(boundary) = self.boundaries.pop() {
            self.labels.truncate(boundary.labels_start as usize);
        }
    }

    /// A class field's initializer, which is a boundary without a scope of its own.
    pub(crate) fn enter_field(&mut self) {
        if self.active() {
            self.boundaries.push(Boundary { labels_start: self.labels.len() as u32, home: Home::Field, loops: 0, switches: 0, in_params: false, simple_params: true });
        }
    }

    pub(crate) fn exit_field(&mut self) {
        if self.active() {
            self.pop_boundary();
        }
    }

    /// The home of a method of the innermost class: its constructor's tells its heritage.
    pub(crate) fn method_home(&self, constructor: bool) -> Home {
        if !constructor {
            Home::Method
        } else if self.classes.last().is_some_and(|class| class.0) {
            Home::DerivedConstructor
        } else {
            Home::BaseConstructor
        }
    }

    pub(crate) fn enter_class_body(&mut self, derived: bool) {
        if self.active() {
            self.classes.push((derived, false));
        }
    }

    /// A constructor of the innermost class: an early error after another.
    pub(crate) fn constructor(&mut self) {
        if let Some(class) = self.classes.last_mut() {
            if class.1 {
                self.unsure = true;
            }
            class.1 = true;
        }
    }

    pub(crate) fn exit_class_body(&mut self) {
        if self.active() {
            self.classes.pop();
        }
    }

    /// Closes the innermost scope: its references to its own names resolve, the rest pass to
    /// the scope around it, and its declarations are checked against one another.
    pub(crate) fn exit(&mut self) {
        if self.scopes.len() <= self.bodies.last().map_or(0, |body| body.scope as usize + 1) {
            // Not a scope of the innermost body (`end_body` closes its function's scope).
            if !self.active() {
                return;
            }
            self.unsure = true;
            return;
        }
        self.resolve_innermost();
        self.close_innermost();
    }

    /// Resolves the innermost scope's references to its own names and checks its
    /// declarations.
    fn resolve_innermost(&mut self) {
        let Some(scope) = self.scopes.last().copied() else { return };
        let decls = &self.decls[scope.decls_start as usize..];
        if !self.unsure && conflicts(decls) {
            self.unsure = true;
        }
        let refs_start = scope.refs_start as usize;
        let mut kept = refs_start;
        let bound = |declared: &(&str, DeclKind)| declared.1 != DeclKind::VarThrough;
        if decls.len() <= 16 {
            for index in refs_start..self.refs.len() {
                let name = self.refs[index];
                if !decls.iter().any(|declared| declared.0 == name && bound(declared)) {
                    self.refs[kept] = name;
                    kept += 1;
                }
            }
        } else {
            let names: rustc_hash::FxHashSet<&str> = decls.iter().filter(|declared| bound(declared)).map(|declared| declared.0).collect();
            for index in refs_start..self.refs.len() {
                let name = self.refs[index];
                if !names.contains(name) {
                    self.refs[kept] = name;
                    kept += 1;
                }
            }
        }
        self.refs.truncate(kept);
    }

    /// Pops the innermost scope, whose references already resolved: its unresolved references
    /// stay for the scope around it, and the `var`s declared in it move there, bound when that
    /// is a function's scope and passing through it otherwise.
    fn close_innermost(&mut self) {
        let Some(scope) = self.scopes.pop() else { return };
        let start = scope.decls_start as usize;
        if scope.kind == ScopeKind::Block && self.decls[start..].iter().any(|(_, kind)| *kind == DeclKind::VarThrough) {
            let kind = if self.scopes.last().is_some_and(|parent| parent.kind == ScopeKind::Function) { DeclKind::Var } else { DeclKind::VarThrough };
            let mut write = start;
            for read in start..self.decls.len() {
                let (name, declared) = self.decls[read];
                if declared == DeclKind::VarThrough {
                    self.decls[write] = (name, kind);
                    write += 1;
                }
            }
            self.decls.truncate(write);
        } else {
            self.decls.truncate(start);
        }
        if self.scopes.is_empty() {
            self.refs.clear();
        }
    }

    #[inline]
    pub(crate) fn reference(&mut self, name: &'a str) {
        if self.active() {
            if reserved(name) {
                self.unsure = true;
            } else if name == "arguments" && self.boundaries.last().is_some_and(|boundary| matches!(boundary.home, Home::Field | Home::StaticBlock)) {
                self.unsure = true;
            }
            if !self.lost {
                self.refs.push(name);
            }
        }
    }

    /// Takes back the last reference to `name`, which turned out to be a binding or a label.
    pub(crate) fn unreference(&mut self, name: &str) {
        if self.active() && !self.lost {
            match self.refs.last() {
                Some(last) if *last == name => {
                    self.refs.pop();
                }
                _ => self.lost = true,
            }
        }
    }

    /// An identifier reference became an assignment target: `eval` and `arguments` may not.
    pub(crate) fn assignment_target(&mut self, name: &str) {
        if self.active() && matches!(name, "eval" | "arguments") {
            self.unsure = true;
        }
    }

    /// Declares `name` in the innermost scope, or for `var` in the innermost function scope,
    /// recorded on each block between.
    pub(crate) fn declare(&mut self, name: &'a str, kind: DeclKind) {
        if !self.active() {
            return;
        }
        if reserved(name) || matches!(name, "eval" | "arguments") {
            self.unsure = true;
        }
        if self.lost {
            return;
        }
        let kind = if kind == DeclKind::Var && self.scopes.last().is_some_and(|scope| scope.kind == ScopeKind::Block) { DeclKind::VarThrough } else { kind };
        self.decls.push((name, kind));
    }

    /// Whether the innermost scope is a block's, where a function declaration is lexical.
    pub(crate) fn in_block(&self) -> bool {
        self.scopes.last().is_some_and(|scope| scope.kind == ScopeKind::Block)
    }

    /// An escaped name: its text may not outlive a released body, so the names tracked from
    /// here are not exact.
    pub(crate) fn escaped_name(&mut self) {
        if self.active() {
            self.lost = true;
        }
    }

    pub(crate) fn mark_unsure(&mut self) {
        if self.active() {
            self.unsure = true;
        }
    }

    pub(crate) fn set_in_params(&mut self, in_params: bool) {
        if let Some(boundary) = self.boundaries.last_mut() {
            boundary.in_params = in_params;
        }
    }

    pub(crate) fn set_simple_params(&mut self, simple: bool) {
        if let Some(boundary) = self.boundaries.last_mut() {
            boundary.simple_params = simple;
        }
    }

    /// A `"use strict"` directive: an early error where the parameters are not simple.
    pub(crate) fn use_strict(&mut self) {
        if self.boundaries.last().is_some_and(|boundary| !boundary.simple_params) {
            self.mark_unsure();
        }
    }

    /// An `await` or `yield` expression: an early error in parameters (and `await` in a
    /// static block).
    pub(crate) fn await_or_yield(&mut self, is_await: bool) {
        if let Some(boundary) = self.boundaries.last()
            && (boundary.in_params || is_await && boundary.home == Home::StaticBlock)
        {
            self.mark_unsure();
        }
    }

    pub(crate) fn super_expression(&mut self, call: bool) {
        let Some(boundary) = self.boundaries.last() else { return };
        let fine = match boundary.home {
            Home::Plain => false,
            Home::DerivedConstructor => true,
            Home::Method | Home::BaseConstructor | Home::Field | Home::StaticBlock => !call,
        };
        if !fine {
            self.mark_unsure();
        }
    }

    pub(crate) fn enter_loop(&mut self) {
        if let Some(boundary) = self.boundaries.last_mut() {
            boundary.loops += 1;
        }
    }

    pub(crate) fn exit_loop(&mut self) {
        if let Some(boundary) = self.boundaries.last_mut() {
            boundary.loops = boundary.loops.saturating_sub(1);
        }
    }

    pub(crate) fn enter_switch(&mut self) {
        if let Some(boundary) = self.boundaries.last_mut() {
            boundary.switches += 1;
        }
    }

    pub(crate) fn exit_switch(&mut self) {
        if let Some(boundary) = self.boundaries.last_mut() {
            boundary.switches = boundary.switches.saturating_sub(1);
        }
    }

    /// Enters a labeled statement's body: a label the function already has around is an
    /// early error.
    pub(crate) fn enter_label(&mut self, name: &'a str, labels_loop: bool) {
        if !self.active() {
            return;
        }
        let start = self.boundaries.last().map_or(0, |boundary| boundary.labels_start as usize);
        if self.labels[start..].iter().any(|(label, _)| *label == name) {
            self.unsure = true;
        }
        self.labels.push((name, labels_loop));
    }

    pub(crate) fn exit_label(&mut self) {
        if self.active() {
            self.labels.pop();
        }
    }

    /// A labeled `break` (`continue` when `continues`): an early error unless a label of
    /// the function around names it (a loop's for `continue`).
    pub(crate) fn labeled_jump(&mut self, name: &str, continues: bool) {
        if !self.active() {
            return;
        }
        let start = self.boundaries.last().map_or(0, |boundary| boundary.labels_start as usize);
        if !self.labels[start..].iter().any(|(label, labels_loop)| *label == name && (*labels_loop || !continues)) {
            self.unsure = true;
        }
    }

    /// An unlabeled `break` (`continue` when `continues`): an early error outside a loop (or
    /// a `switch` for `break`).
    pub(crate) fn jump(&mut self, continues: bool) {
        if let Some(boundary) = self.boundaries.last()
            && boundary.loops == 0
            && (continues || boundary.switches == 0)
        {
            self.mark_unsure();
        }
    }
}

/// Whether a scope's declarations may conflict: a name declared twice where either declaration
/// is lexical, two catch parameters, or two parameters.
fn conflicts(decls: &[(&str, DeclKind)]) -> bool {
    let clash = |a: DeclKind, b: DeclKind| {
        if a == DeclKind::OwnName || b == DeclKind::OwnName {
            return false;
        }
        a == DeclKind::Lexical
            || b == DeclKind::Lexical
            || a == DeclKind::CatchParam && b == DeclKind::CatchParam
            || a == DeclKind::Param && b == DeclKind::Param
    };
    if decls.len() <= 16 {
        return decls.iter().enumerate().any(|(index, (name, kind))| {
            decls[index + 1..].iter().any(|(other, other_kind)| other == name && clash(*kind, *other_kind))
        });
    }
    // Per name, the kinds seen so far as bits: a later kind clashes with any of them.
    let mut seen: rustc_hash::FxHashMap<&str, u8> = rustc_hash::FxHashMap::default();
    seen.reserve(decls.len());
    let lexical = 1u8 << (DeclKind::Lexical as u8);
    let own = 1u8 << (DeclKind::OwnName as u8);
    for (name, kind) in decls {
        let bit = 1u8 << (*kind as u8);
        let kinds = seen.entry(name).or_insert(0);
        let others = *kinds & !own;
        if *kind != DeclKind::OwnName
            && others != 0
            && (*kind == DeclKind::Lexical
                || others & lexical != 0
                || matches!(kind, DeclKind::CatchParam | DeclKind::Param) && others & bit != 0)
        {
            return true;
        }
        *kinds |= bit;
    }
    false
}

/// Whether a parameter list is simple: plain names only, no defaults, no rest.
pub(crate) fn simple_parameters(params: &oxc_ast::ast::FormalParameters<'_>) -> bool {
    params.rest.is_none()
        && params.items.iter().all(|param| param.initializer.is_none() && matches!(param.pattern, oxc_ast::ast::BindingPattern::BindingIdentifier(_)))
}

impl<'a, C: crate::ParserConfig> crate::ParserImpl<'a, C> {
    /// Declares a binding identifier as the construct around it says (`binding_kind`).
    #[inline]
    pub(crate) fn declare_binding(&mut self, name: &'a str) {
        if let Some(kind) = self.binding_kind {
            self.early.declare(name, kind);
        }
    }
}
