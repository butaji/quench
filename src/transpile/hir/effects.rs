//! Effect inference for functions
//!
//! Analyzes function bodies to determine:
//! - Whether a function throws (error effect)
//! - The type of error that can be thrown

use super::{Block, Expr, FunctionDecl, Stmt, Type};

/// Context for effect analysis
struct EffectAnalyzer {
    /// Whether the current scope can throw
    can_throw: bool,
    /// Types of errors that can be thrown
    error_types: Vec<Type>,
}

impl EffectAnalyzer {
    fn new() -> Self {
        Self {
            can_throw: false,
            error_types: Vec::new(),
        }
    }

    /// Analyze a function and set throws/error_type
    fn analyze_function(&mut self, func: &mut FunctionDecl) {
        self.can_throw = false;
        self.error_types.clear();

        if let Some(body) = &func.body {
            for s in &body.0 {
                self.analyze_stmt(s);
            }
        }

        func.throws = self.can_throw;
        func.error_type = if self.can_throw {
            Some(self.infer_error_type())
        } else {
            None
        };
    }

    /// Infer the error type from collected error types
    fn infer_error_type(&self) -> Type {
        // If we have specific error types, use the first one
        // In a more sophisticated system, we'd compute a union type
        self.error_types.first().cloned().unwrap_or(Type::Unknown)
    }

    /// Analyze a statement for effects
    fn analyze_stmt(&mut self, stmt: &Stmt) {
        self.analyze_throw_stmt(stmt);
        self.analyze_expr_stmt(stmt);
        self.analyze_block_stmt(stmt);
        self.analyze_control_flow(stmt);
    }

    fn analyze_throw_stmt(&mut self, stmt: &Stmt) {
        if let Stmt::Throw { arg } = stmt {
            self.can_throw = true;
            self.error_types.push(self.infer_type_from_expr(arg));
        }
    }
    fn analyze_expr_stmt(&mut self, stmt: &Stmt) {
        if let Stmt::Expr { expr } = stmt {
            self.analyze_expr(expr);
        }
    }
    fn analyze_block_stmt(&mut self, stmt: &Stmt) {
        if let Stmt::Block(stmts) = stmt {
            self.analyze_block(stmts);
        }
    }

    fn analyze_control_flow(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::If {
                test,
                consequent,
                alternate,
            } => self.analyze_if(test, consequent, alternate),
            S::While { test, body } => self.analyze_while(test, body),
            S::For {
                init,
                test,
                update,
                body,
            } => self.analyze_for(init, test, update, body),
            S::Return { arg } => self.analyze_return(arg),
            S::Try {
                block,
                handler,
                finalizer,
            } => self.analyze_try(block, handler, finalizer),
            S::Switch {
                discriminant,
                cases,
            } => self.analyze_switch(discriminant, cases),
            _ => {}
        }
    }

    fn analyze_throw(&mut self, arg: &Option<Expr>) {
        self.can_throw = true;
        if let Some(e) = arg {
            self.error_types.push(self.infer_type_from_expr(e));
        }
    }

    fn analyze_block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.analyze_stmt(s);
        }
    }

    fn analyze_if(&mut self, test: &Expr, cons: &Box<Stmt>, alt: &Option<Box<Stmt>>) {
        self.analyze_expr(test);
        self.analyze_stmt(cons);
        if let Some(a) = alt {
            self.analyze_stmt(a);
        }
    }

    fn analyze_while(&mut self, test: &Expr, body: &Box<Stmt>) {
        self.analyze_expr(test);
        self.analyze_stmt(body);
    }

    fn analyze_for(
        &mut self,
        init: &Option<super::ForInit>,
        test: &Option<Expr>,
        update: &Option<Expr>,
        body: &Box<Stmt>,
    ) {
        self.analyze_for_init(init);
        if let Some(t) = test {
            self.analyze_expr(t);
        }
        if let Some(u) = update {
            self.analyze_expr(u);
        }
        self.analyze_stmt(body);
    }

    fn analyze_return(&mut self, arg: &Option<Expr>) {
        if let Some(e) = arg {
            self.analyze_expr(e);
        }
    }

    fn analyze_try(
        &mut self,
        block: &Block,
        handler: &Option<super::CatchClause>,
        finalizer: &Option<Block>,
    ) {
        for s in &block.0 {
            self.analyze_stmt(s);
        }
        if let Some(h) = handler {
            for s in &h.body.0 {
                self.analyze_stmt(s);
            }
        }
        if let Some(f) = finalizer {
            for s in &f.0 {
                self.analyze_stmt(s);
            }
        }
    }

    fn analyze_switch(&mut self, discriminant: &Expr, cases: &[super::SwitchCase]) {
        self.analyze_expr(discriminant);
        for case in cases {
            for s in &case.consequent {
                self.analyze_stmt(s);
            }
        }
    }

    fn analyze_for_init(&mut self, init: &Option<super::ForInit>) {
        if let Some(super::ForInit::Expr(e)) = init {
            self.analyze_expr(e);
        }
    }

    /// Analyze an expression for effects
    fn analyze_expr(&mut self, expr: &Expr) {
        self.analyze_call_expr(expr);
        self.analyze_await_expr(expr);
        self.analyze_binary_expr(expr);
        self.analyze_container_expr(expr);
        self.analyze_assign_expr(expr);
    }

    fn analyze_call_expr(&mut self, expr: &Expr) {
        if let Expr::Call { callee, arguments } = expr {
            self.analyze_call(callee, arguments);
        }
        if let Expr::New { callee, arguments } = expr {
            self.analyze_new(callee, arguments);
        }
    }

    fn analyze_await_expr(&mut self, expr: &Expr) {
        if let Expr::Await { arg } = expr {
            self.analyze_await(arg);
        }
    }

    fn analyze_binary_expr(&mut self, expr: &Expr) {
        if let Expr::Bin { left, right, .. } = expr {
            self.analyze_bin(left, right);
        }
        if let Expr::Logical { left, right, .. } = expr {
            self.analyze_logical(left, right);
        }
        if let Expr::Cond {
            test,
            consequent,
            alternate,
        } = expr
        {
            self.analyze_cond(test, consequent, alternate);
        }
    }

    fn analyze_container_expr(&mut self, expr: &Expr) {
        if let Expr::Array { elems } = expr {
            self.analyze_array(elems);
        }
        if let Expr::Object { members } = expr {
            self.analyze_object(members);
        }
        if let Expr::Seq { left, right } = expr {
            self.analyze_expr(left);
            self.analyze_expr(right);
        }
    }

    fn analyze_assign_expr(&mut self, expr: &Expr) {
        if let Expr::Assign { left, right, .. } = expr {
            self.analyze_assign(left, right);
        }
    }

    fn analyze_call(&mut self, callee: &Expr, args: &[Expr]) {
        for a in args {
            self.analyze_expr(a);
        }
        self.analyze_expr(callee);
    }
    fn analyze_new(&mut self, callee: &Expr, args: &[Expr]) {
        for a in args {
            self.analyze_expr(a);
        }
        self.analyze_expr(callee);
    }
    fn analyze_await(&mut self, arg: &Box<Expr>) {
        self.analyze_expr(arg);
        self.can_throw = true;
    }
    fn analyze_bin(&mut self, l: &Expr, r: &Expr) {
        self.analyze_expr(l);
        self.analyze_expr(r);
    }
    fn analyze_logical(&mut self, l: &Expr, r: &Expr) {
        self.analyze_expr(l);
        self.analyze_expr(r);
    }
    fn analyze_cond(&mut self, t: &Expr, c: &Expr, a: &Expr) {
        self.analyze_expr(t);
        self.analyze_expr(c);
        self.analyze_expr(a);
    }
    fn analyze_array(&mut self, elems: &[Option<Expr>]) {
        for e in elems {
            if let Some(x) = e {
                self.analyze_expr(x);
            }
        }
    }
    fn analyze_object(&mut self, members: &[super::ObjectMemberExpr]) {
        for m in members {
            if let super::ObjectProp::Init { value, .. } = &m.prop {
                self.analyze_expr(value);
            }
        }
    }
    fn analyze_assign(&mut self, l: &Expr, r: &Expr) {
        self.analyze_expr(l);
        self.analyze_expr(r);
    }
    fn analyze_seq(&mut self, exprs: &[Expr]) {
        for e in exprs {
            self.analyze_expr(e);
        }
    }

    /// Infer a type from an expression (for error types)
    fn infer_type_from_expr(&self, expr: &Expr) -> Type {
        match expr {
            Expr::New { callee, .. } => Type::Ref {
                name: format!("{}Error", self.type_name_from_callee(callee)),
                generics: vec![],
            },
            Expr::Ident { name } => Type::Ref {
                name: name.clone(),
                generics: vec![],
            },
            Expr::StaticMember { property, .. } => {
                if property == "TypeError" || property == "Error" || property == "RangeError" {
                    Type::Ref {
                        name: property.clone(),
                        generics: vec![],
                    }
                } else {
                    Type::Unknown
                }
            }
            _ => Type::Unknown,
        }
    }

    fn type_name_from_callee(&self, callee: &Expr) -> String {
        match callee {
            Expr::Ident { name } => name.clone(),
            Expr::StaticMember { property, .. } => property.clone(),
            _ => "Error".to_string(),
        }
    }
}

impl EffectAnalyzer {
    /// Analyze a function for effects (throw, async, etc.)
    pub fn analyze_function_copy(&self, func: &FunctionDecl) -> (bool, Option<Type>) {
        let mut can_throw = false;
        let mut error_types = Vec::new();

        if let Some(body) = &func.body {
            for s in &body.0 {
                self.analyze_stmt_inner(s, &mut can_throw, &mut error_types);
            }
        }

        let error_type = error_types.first().cloned();
        (can_throw, error_type)
    }

    /// Inner analysis with mutable state
    fn analyze_stmt_inner(&self, stmt: &Stmt, can_throw: &mut bool, error_types: &mut Vec<Type>) {
        if let Stmt::Throw { arg } = stmt {
            *can_throw = true;
            error_types.push(self.infer_type_from_expr(arg));
        }
        if let Stmt::Expr { expr } = stmt {
            self.analyze_expr_inner(expr, can_throw, error_types);
        }
        if let Stmt::Block(stmts) = stmt {
            for s in stmts {
                self.analyze_stmt_inner(s, can_throw, error_types);
            }
        }
        self.analyze_control_flow_inner(stmt, can_throw, error_types);
    }

    fn analyze_control_flow_inner(
        &self,
        stmt: &Stmt,
        can_throw: &mut bool,
        error_types: &mut Vec<Type>,
    ) {
        self.analyze_if_inner(stmt, can_throw, error_types);
        self.analyze_while_inner(stmt, can_throw, error_types);
        self.analyze_return_inner(stmt, can_throw, error_types);
        self.analyze_try_inner(stmt, can_throw, error_types);
    }

    fn analyze_if_inner(&self, stmt: &Stmt, can_throw: &mut bool, error_types: &mut Vec<Type>) {
        if let Stmt::If {
            test,
            consequent,
            alternate,
        } = stmt
        {
            self.analyze_expr_inner(test, can_throw, error_types);
            self.analyze_stmt_inner(consequent, can_throw, error_types);
            if let Some(a) = alternate {
                self.analyze_stmt_inner(a, can_throw, error_types);
            }
        }
    }
    fn analyze_while_inner(&self, stmt: &Stmt, can_throw: &mut bool, error_types: &mut Vec<Type>) {
        if let Stmt::While { test, body } = stmt {
            self.analyze_expr_inner(test, can_throw, error_types);
            self.analyze_stmt_inner(body, can_throw, error_types);
        }
    }
    fn analyze_return_inner(&self, stmt: &Stmt, can_throw: &mut bool, error_types: &mut Vec<Type>) {
        if let Stmt::Return { arg } = stmt {
            if let Some(e) = arg {
                self.analyze_expr_inner(e, can_throw, error_types);
            }
        }
    }
    fn analyze_try_inner(&self, stmt: &Stmt, can_throw: &mut bool, error_types: &mut Vec<Type>) {
        if let Stmt::Try {
            block,
            handler,
            finalizer,
        } = stmt
        {
            for s in &block.0 {
                self.analyze_stmt_inner(s, can_throw, error_types);
            }
            if let Some(h) = handler {
                for s in &h.body.0 {
                    self.analyze_stmt_inner(s, can_throw, error_types);
                }
            }
            if let Some(f) = finalizer {
                for s in &f.0 {
                    self.analyze_stmt_inner(s, can_throw, error_types);
                }
            }
        }
    }

    fn analyze_expr_inner(&self, expr: &Expr, can_throw: &mut bool, error_types: &mut Vec<Type>) {
        if let Expr::Await { .. } = expr {
            *can_throw = true;
        }
        // For simplicity, just recurse into sub-expressions
        if let Expr::Call { callee, arguments } = expr {
            for a in arguments {
                self.analyze_expr_inner(a, can_throw, error_types);
            }
            self.analyze_expr_inner(callee, can_throw, error_types);
        }
    }
}

/// Analyze a function for effects (throw, async, etc.)
pub fn analyze_effects(func: &FunctionDecl) -> (bool, Option<Type>) {
    let analyzer = EffectAnalyzer::new();
    analyzer.analyze_function_copy(func)
}

/// Analyze all functions in a module for effects
pub fn analyze_module_effects(stmts: &[Stmt]) {
    for stmt in stmts {
        if let Stmt::FunctionDecl(func) = stmt {
            let _ = analyze_effects(func);
        }
    }
}
