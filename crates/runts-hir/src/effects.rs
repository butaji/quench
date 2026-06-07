//! Effect inference for functions
//!
//! Analyzes function bodies to determine:
//! - Whether a function throws (error effect)
//! - The type of error that can be thrown
//!

use super::{Block, ClassMethod, Expr, FunctionDecl, Stmt, Type};

/// Context for effect analysis
#[allow(dead_code)]
pub struct EffectAnalyzer {
    /// Whether the current scope can throw
    can_throw: bool,
    /// Types of errors that can be thrown
    error_types: Vec<Type>,
    /// Functions that are known to throw
    known_throw_funcs: std::collections::HashSet<String>,
}

#[allow(dead_code)]
impl EffectAnalyzer {
    pub fn new() -> Self {
        Self {
            can_throw: false,
            error_types: Vec::new(),
            known_throw_funcs: std::collections::HashSet::new(),
        }
    }

    /// Analyze a function and set throws/error_type
    pub fn analyze_function(&mut self, func: &mut FunctionDecl) {
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

    /// Infer the error type from collected error types - compute union
    fn infer_error_type(&self) -> Type {
        if self.error_types.is_empty() {
            return Type::Unknown;
        }
        if self.error_types.len() == 1 {
            return self.error_types.first().cloned().unwrap_or(Type::Unknown);
        }
        // Compute union of all error types
        Type::Union {
            types: self.error_types.clone(),
        }
    }

    /// Analyze a statement for effects
    fn analyze_stmt(&mut self, stmt: &Stmt) {
        // Note: analyze_control_flow handles Throw statements,
        // so we don't call analyze_throw_stmt separately
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
        if let Stmt::Block { stmts } = stmt {
            self.analyze_block(stmts);
        }
    }

    fn analyze_control_flow(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::If{test,consequent,alternate}=>self.analyze_if(test,consequent,alternate),
            S::While{test: _,body: _}|S::DoWhile{body: _,test: _}=>self.analyze_cf_while(stmt),
            S::For{..}|S::ForIn{..}|S::ForOf{..}=>self.analyze_cf_loop(stmt),
            S::Return{..}|S::Throw{..}=>self.analyze_cf_control(stmt),
            S::Try{..}|S::Switch{..}=>self.analyze_cf_complex(stmt),
            S::Break{..}|S::Continue{..}|S::Labeled{..}|S::Empty|S::FunctionDecl(_)|S::Class(_)|S::Variable(_)|S::ExportNamed{..}|S::ExportDefault{..}|S::ImportNamed{..}|S::ImportDefault{..}=>{}
            S::With{obj,body}=>self.analyze_with(obj,body),
            S::Block{stmts:_}=>self.analyze_cf_block(stmt),
            S::Expr{expr:_}=>self.analyze_cf_block(stmt),
        }
    }

    fn analyze_cf_while(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::While { test, body } => self.analyze_while(test, body),
            S::DoWhile { body, test } => self.analyze_dowhile(body, test),
            _ => {}
        }
    }

    fn analyze_cf_loop(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::For { init, test, update, body } => self.analyze_for(init, test, update, body),
            S::ForIn { left, right, body } => self.analyze_forin(left, right, body),
            S::ForOf { left, right, body, is_await } => self.analyze_forof(left, right, body, *is_await),
            _ => {}
        }
    }

    fn analyze_cf_complex(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Try { block, handler, finalizer } => self.analyze_try(block, handler, finalizer),
            S::Switch { discriminant, cases } => self.analyze_switch(discriminant, cases),
            _ => {}
        }
    }

    fn analyze_cf_block(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Block { stmts } => self.analyze_block(stmts),
            S::Expr { expr } => self.analyze_expr(expr),
            _ => {}
        }
    }

    fn analyze_cf_control(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Return { arg } => self.analyze_return(arg),
            S::Throw { arg } => self.analyze_throw(arg),
            _ => {}
        }
    }

    fn analyze_forin(&mut self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>) {
        self.analyze_for_init(Some(left));
        self.analyze_expr(right);
        self.analyze_stmt(body);
    }

    fn analyze_forof(&mut self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>, is_await: bool) {
        self.analyze_for_init(Some(left));
        self.analyze_expr(right);
        if is_await {
            self.analyze_await_expr(&Expr::Await { arg: Box::new(right.clone()) });
        }
        self.analyze_stmt(body);
    }

    fn analyze_dowhile(&mut self, body: &Box<Stmt>, test: &Expr) {
        self.analyze_stmt(body);
        self.analyze_expr(test);
    }

    fn analyze_with(&mut self, obj: &Expr, body: &Box<Stmt>) {
        self.analyze_expr(obj);
        self.analyze_stmt(body);
    }

    fn analyze_throw(&mut self, arg: &Expr) {
        self.can_throw = true;
        self.error_types.push(self.infer_type_from_expr(arg));
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
        self.analyze_for_init(init.as_ref());
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

    fn analyze_for_init(&mut self, init: Option<&super::ForInit>) {
        if let Some(super::ForInit::Expr(e)) = init {
            self.analyze_expr(e);
        }
        if let Some(super::ForInit::Variable(_, vars)) = init {
            for (_, init_expr) in vars {
                if let Some(e) = init_expr {
                    self.analyze_expr(e);
                }
            }
        }
    }

    /// Analyze an expression for effects
    fn analyze_expr(&mut self, expr: &Expr) {
        self.analyze_call_expr(expr);
        self.analyze_await_expr(expr);
        self.analyze_binary_expr(expr);
        self.analyze_container_expr(expr);
        self.analyze_assign_expr(expr);
        self.analyze_unary_expr(expr);
    }

    fn analyze_call_expr(&mut self, expr: &Expr) {
        if let Expr::Call { callee, arguments } = expr {
            // Check if this function is known to throw
            if let Expr::Ident { name } = callee.as_ref() {
                if self.known_throw_funcs.contains(name) {
                    self.can_throw = true;
                    self.error_types.push(Type::Ref {
                        name: format!("{}Error", name),
                        generics: vec![],
                    });
                }
            }
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

    fn analyze_unary_expr(&mut self, expr: &Expr) {
        if let Expr::Unary { arg, op, .. } = expr {
            // delete can throw in strict mode
            if matches!(op, super::UnaryOp::Delete) {
                self.can_throw = true;
            }
            self.analyze_expr(arg);
        }
    }

    fn analyze_binary_expr(&mut self, expr: &Expr) {
        if let Expr::Bin { left, right, .. } = expr {
            self.analyze_expr(left);
            self.analyze_expr(right);
        }
        if let Expr::Logical { left, right, .. } = expr {
            self.analyze_expr(left);
            self.analyze_expr(right);
        }
        if let Expr::Cond { test, consequent, alternate } = expr {
            self.analyze_expr(test);
            self.analyze_expr(consequent);
            self.analyze_expr(alternate);
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
        if let Expr::Spread { arg } = expr {
            self.analyze_expr(arg);
        }
        if let Expr::Template { exprs, .. } = expr {
            for e in exprs {
                self.analyze_expr(e);
            }
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

    /// Analyze await - only sets can_throw if the awaited expression can throw
    fn analyze_await(&mut self, arg: &Box<Expr>) {
        self.analyze_expr(arg);
        // Await can only throw if the awaited expression is a call that throws
        // For now, we conservatively assume awaits of potentially-throwing calls can throw
        if let Expr::Call { .. } = arg.as_ref() {
            self.can_throw = true;
            self.error_types.push(Type::Ref {
                name: "JsValue".to_string(),
                generics: vec![],
            });
        }
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
            match &m.prop {
                super::ObjectProp::Init { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Get { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Set { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Method { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Spread { arg } => self.analyze_expr(arg),
            }
        }
    }
    fn analyze_assign(&mut self, l: &Expr, r: &Expr) {
        self.analyze_expr(l);
        self.analyze_expr(r);
    }

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
            Expr::StaticMember { property, .. } => self.infer_error_type_from_property(property),
            Expr::String(s) => Type::Literal {
                kind: super::LiteralKind::String,
                value: s.clone(),
            },
            Expr::Number(n) => Type::Literal {
                kind: super::LiteralKind::Number,
                value: n.to_string(),
            },
            Expr::Boolean(b) => Type::Literal {
                kind: super::LiteralKind::Boolean,
                value: b.to_string(),
            },
            _ => Type::Unknown,
        }
    }

    fn infer_error_type_from_property(&self, property: &str) -> Type {
        if property == "TypeError" || property == "Error" || property == "RangeError" {
            Type::Ref {
                name: property.to_string(),
                generics: vec![],
            }
        } else {
            Type::Unknown
        }
    }

    fn type_name_from_callee(&self, callee: &Expr) -> String {
        match callee {
            Expr::Ident { name } => name.clone(),
            Expr::StaticMember { property, .. } => property.clone(),
            _ => "Error".to_string(),
        }
    }

    /// Register a function as known to throw
    fn register_throw_func(&mut self, name: &str) {
        self.known_throw_funcs.insert(name.to_string());
    }
}

/// Analyze a function for effects (throw, async, etc.)
pub fn analyze_effects(func: &FunctionDecl) -> (bool, Option<Type>) {
    let mut analyzer = EffectAnalyzer::new();
    analyzer.analyze_function(&mut func.clone());
    (analyzer.can_throw, analyzer.error_types.first().cloned())
}

/// Analyze a class method for effects
/// For now, we conservatively assume methods don't throw
#[allow(dead_code)]
pub fn analyze_method_effects(_method: &ClassMethod) -> (bool, Option<Type>) {
    // For now, conservatively assume no effects
    (false, None)
}

/// Analyze all functions in a module for effects
#[allow(dead_code)]
pub fn analyze_module_effects(stmts: &[Stmt]) {
    for stmt in stmts {
        if let Stmt::FunctionDecl(func) = stmt {
            let mut analyzer = EffectAnalyzer::new();
            let mut func_clone = func.clone();
            analyzer.analyze_function(&mut func_clone);
        }
    }
}
