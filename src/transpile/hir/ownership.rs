//! Ownership inference for functions
//!
//! Analyzes function bodies to determine the ownership of parameters:
//! - Borrow (&T): function only reads the parameter
//! - Mut (&mut T): function mutates the parameter
//! - Owned (T): function takes ownership

use super::{Expr, FunctionDecl, Ownership, Param, Stmt};

/// Context for ownership analysis
struct OwnershipAnalyzer {
    /// Variables that are mutably borrowed in current scope
    mut_vars: std::collections::HashSet<String>,
    /// Variables that are immutably borrowed in current scope
    borrow_vars: std::collections::HashSet<String>,
}

impl OwnershipAnalyzer {
    fn new() -> Self {
        Self {
            mut_vars: std::collections::HashSet::new(),
            borrow_vars: std::collections::HashSet::new(),
        }
    }

    /// Analyze a function and return ownership for each parameter
    fn analyze_function(&mut self, func: &FunctionDecl) -> Vec<Ownership> {
        // Reset state
        self.mut_vars.clear();
        self.borrow_vars.clear();

        // Analyze the body if present
        if let Some(body) = &func.body {
            for s in &body.0 {
                self.analyze_stmt(s);
            }
        }

        // Determine ownership based on usage
        func.params
            .iter()
            .map(|p| self.param_ownership(&p.name))
            .collect()
    }

    /// Determine ownership for a parameter based on its usage
    fn param_ownership(&self, name: &str) -> Ownership {
        if self.mut_vars.contains(name) {
            Ownership::Mut
        } else if self.borrow_vars.contains(name) {
            Ownership::Borrow
        } else {
            Ownership::Owned
        }
    }

    /// Analyze a statement and update borrow state
    fn analyze_stmt(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Expr { expr } => self.analyze_expr(expr),
            S::Block(stmts) => self.analyze_block(stmts),
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
            _ => {}
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

    fn analyze_for_init(&mut self, init: &Option<super::ForInit>) {
        if let Some(super::ForInit::Expr(e)) = init {
            self.analyze_expr(e);
        }
    }

    /// Analyze an expression and update borrow state
    fn analyze_expr(&mut self, expr: &Expr) {
        self.analyze_assign_expr(expr);
        self.analyze_call_expr(expr);
        self.analyze_container_expr(expr);
    }

    fn analyze_assign_expr(&mut self, expr: &Expr) {
        if let Expr::Assign { op, left, right } = expr {
            self.analyze_assign(op, left, right);
        }
    }

    fn analyze_call_expr(&mut self, expr: &Expr) {
        if let Expr::Member { obj, .. } = expr {
            self.analyze_expr(obj);
        }
        if let Expr::StaticMember { obj, .. } = expr {
            self.analyze_expr(obj);
        }
        if let Expr::Call { callee, arguments } = expr {
            self.analyze_call(callee, arguments);
        }
    }

    fn analyze_container_expr(&mut self, expr: &Expr) {
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
        if let Expr::Array { elems } = expr {
            self.analyze_array(elems);
        }
        if let Expr::Object { members } = expr {
            self.analyze_object(members);
        }
    }

    fn analyze_call(&mut self, callee: &Expr, args: &[Expr]) {
        self.analyze_expr(callee);
        for a in args {
            self.analyze_expr(a);
        }
    }
    fn analyze_bin(&mut self, left: &Expr, right: &Expr) {
        self.analyze_expr(left);
        self.analyze_expr(right);
    }
    fn analyze_logical(&mut self, left: &Expr, right: &Expr) {
        self.analyze_expr(left);
        self.analyze_expr(right);
    }
    fn analyze_cond(&mut self, test: &Expr, cons: &Expr, alt: &Expr) {
        self.analyze_expr(test);
        self.analyze_expr(cons);
        self.analyze_expr(alt);
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

    /// Analyze an assignment and determine if it's a mutation
    fn analyze_assign(&mut self, op: &super::AssignOp, left: &Expr, right: &Expr) {
        self.analyze_expr(right);
        // Check if this is a mutation
        if self.is_compound_assign(op) || self.is_member_mutation(left) {
            if let Expr::Ident { name } = left {
                self.mut_vars.insert(name.clone());
            } else if let Expr::Member { obj, .. } = left {
                self.extract_mut_name(obj);
            } else if let Expr::StaticMember { obj, .. } = left {
                self.extract_mut_name(obj);
            }
        }
    }
    fn is_compound_assign(&self, op: &super::AssignOp) -> bool {
        use super::AssignOp as A;
        matches!(
            op,
            A::AddAssign | A::SubAssign | A::MulAssign | A::DivAssign | A::ModAssign
        )
    }
    fn is_member_mutation(&self, left: &Expr) -> bool {
        matches!(left, Expr::Member { .. } | Expr::StaticMember { .. })
    }
    fn extract_mut_name(&mut self, obj: &Expr) {
        if let Expr::Ident { name } = obj {
            self.mut_vars.insert(name.clone());
        }
    }
}

/// Infer ownership annotations for all function parameters
pub fn infer_function_ownership(func: &mut FunctionDecl) {
    let mut analyzer = OwnershipAnalyzer::new();
    let ownerships = analyzer.analyze_function(func);

    for (param, ownership) in func.params.iter_mut().zip(ownerships.into_iter()) {
        param.ownership = ownership;
    }
}

/// Infer ownership for all functions in a module
#[allow(dead_code)]
pub fn infer_module_ownership(stmts: &[Stmt]) -> Vec<Ownership> {
    let mut result = Vec::new();
    for stmt in stmts {
        if let Stmt::FunctionDecl(func) = stmt {
            let mut analyzer = OwnershipAnalyzer::new();
            result.extend(analyzer.analyze_function(func));
        }
    }
    result
}

/// Get a summary of parameter ownership for documentation/debugging
#[allow(dead_code)]
pub fn ownership_summary(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| format!("{}: {:?}", p.name, p.ownership))
        .collect::<Vec<_>>()
        .join(", ")
}
