//! Ownership inference for functions
//!
//! Analyzes function bodies to determine the ownership of parameters:
//! - Borrow (&T): function only reads the parameter
//! - Mut (&mut T): function mutates the parameter
//! - Owned (T): function takes ownership
//!

use super::{ClassMethod, Expr, FunctionDecl, Ownership, Param, Stmt, VariableKind};

/// Context for ownership analysis
pub(crate) struct OwnershipAnalyzer {
    /// Variables that are mutably borrowed in current scope
    mut_vars: std::collections::HashSet<String>,
    /// Variables that are immutably borrowed in current scope
    borrow_vars: std::collections::HashSet<String>,
    /// Aliases: maps a variable to variables that may point to it
    aliases: std::collections::HashMap<String, std::collections::HashSet<String>>,
    /// Current scope variables and their kinds
    scope_vars: std::collections::HashMap<String, VariableKind>,
}

impl OwnershipAnalyzer {
    pub(crate) fn new() -> Self {
        Self {
            mut_vars: std::collections::HashSet::new(),
            borrow_vars: std::collections::HashSet::new(),
            aliases: std::collections::HashMap::new(),
            scope_vars: std::collections::HashMap::new(),
        }
    }

    /// Analyze a function and return ownership for each parameter
    pub(crate) fn analyze_function(&mut self, func: &FunctionDecl) -> Vec<Ownership> {
        // Reset state
        self.mut_vars.clear();
        self.borrow_vars.clear();
        self.aliases.clear();
        self.scope_vars.clear();

        // Register function parameters as scope variables
        for param in &func.params {
            self.scope_vars.insert(param.name.clone(), VariableKind::Let);
        }

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

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Expr { expr } => self.analyze_expr_block(stmt),
            S::Block { stmts: _ } => self.analyze_expr_block(stmt),
            S::If { test, consequent, alternate } => self.analyze_if(test, consequent, alternate),
            S::While { test, body } => self.analyze_while_do(test, body),
            S::DoWhile { body, test } => self.analyze_while_do(test, body),
            S::For { .. } | S::ForIn { .. } | S::ForOf { .. } => self.analyze_stmt_for(stmt),
            S::Return { arg } => self.analyze_return_throw(arg.as_ref()),
            S::Throw { arg } => self.analyze_return_throw(Some(arg)),
            S::Switch { discriminant, cases } => self.analyze_switch_try(stmt),
            S::Try { block, handler, finalizer } => self.analyze_try(block, handler, finalizer),
            S::Break { .. } | S::Continue { .. } | S::Labeled { .. } | S::With { .. } => {}
            _ => {}
        }
    }

    fn analyze_expr_block(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Expr { expr } => self.analyze_expr(expr),
            S::Block { stmts } => self.analyze_block(stmts),
            _ => {}
        }
    }

    fn analyze_switch_try(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::Switch { discriminant, cases } => self.analyze_switch(discriminant, cases),
            S::Try { block, handler, finalizer } => self.analyze_try(block, handler, finalizer),
            _ => {}
        }
    }

    fn analyze_while_do(&mut self, test: &Expr, body: &Box<Stmt>) {
        self.analyze_while(test, body);
    }

    fn analyze_stmt_for(&mut self, stmt: &Stmt) {
        use Stmt as S;
        match stmt {
            S::For { init, test, update, body } => self.analyze_for(init, test, update, body),
            S::ForIn { left, right, body } => self.analyze_for_in(left, right, body),
            S::ForOf { left, right, body, .. } => self.analyze_for_of(left, right, body),
            _ => {}
        }
    }

    fn analyze_return_throw(&mut self, arg: Option<&Expr>) {
        if let Some(a) = arg {
            self.analyze_expr(a);
        }
    }

    fn analyze_with_stmt(&mut self, obj: &Expr, body: &Box<Stmt>) {
        self.analyze_expr(obj);
        self.analyze_stmt(body);
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
    fn analyze_do_while(&mut self, body: &Box<Stmt>, test: &Expr) {
        self.analyze_stmt(body);
        self.analyze_expr(test);
    }
    fn analyze_for(
        &mut self,
        init: &Option<super::ForInit>,
        test: &Option<Expr>,
        update: &Option<Expr>,
        body: &Box<Stmt>,
    ) {
        self.analyze_for_init(init.as_ref().map(|f| f as &super::ForInit));
        if let Some(t) = test {
            self.analyze_expr(t);
        }
        if let Some(u) = update {
            self.analyze_expr(u);
        }
        self.analyze_stmt(body);
    }
    fn analyze_for_in(&mut self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>) {
        self.analyze_for_init(Some(left));
        self.analyze_expr(right);
        self.analyze_stmt(body);
    }
    fn analyze_for_of(&mut self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>) {
        self.analyze_for_init(Some(left));
        self.analyze_expr(right);
        self.analyze_stmt(body);
    }
    fn analyze_switch(&mut self, discriminant: &Expr, cases: &[super::SwitchCase]) {
        self.analyze_expr(discriminant);
        for case in cases {
            for s in &case.consequent {
                self.analyze_stmt(s);
            }
        }
    }
    fn analyze_return(&mut self, arg: &Option<Expr>) {
        if let Some(e) = arg {
            self.analyze_expr(e);
        }
    }
    fn analyze_try(&mut self, block: &super::Block, handler: &Option<super::CatchClause>, finalizer: &Option<super::Block>) {
        for s in &block.0 {
            self.analyze_stmt(s);
        }
        if let Some(h) = handler {
            // Catch parameter is a new scope
            self.scope_vars.insert(h.param.clone(), VariableKind::Let);
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

    fn analyze_for_init(&mut self, init: Option<&super::ForInit>) {
        match init {
            Some(super::ForInit::Variable(kind, vars)) => {
                for (name, init_expr) in vars {
                    self.scope_vars.insert(name.clone(), kind.clone());
                    if let Some(e) = init_expr {
                        self.analyze_expr(e);
                    }
                }
            }
            Some(super::ForInit::Expr(e)) => {
                self.analyze_expr(e);
            }
            None => {}
        }
    }

    /// Analyze an expression and update borrow state
    fn analyze_expr(&mut self, expr: &Expr) {
        self.analyze_assign_expr(expr);
        self.analyze_call_expr(expr);
        self.analyze_container_expr(expr);
        self.analyze_update_expr(expr);
        self.analyze_await_expr(expr);
    }

    fn analyze_assign_expr(&mut self, expr: &Expr) {
        if let Expr::Assign { op, left, right } = expr {
            self.analyze_assign(op, left, right);
        }
    }

    fn analyze_update_expr(&mut self, expr: &Expr) {
        if let Expr::Update { arg, .. } = expr {
            self.mark_as_mutated(arg);
        }
    }

    fn analyze_await_expr(&mut self, expr: &Expr) {
        if let Expr::Await { arg } = expr {
            // Await doesn't mutate, but we analyze the argument
            self.analyze_expr(arg);
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
        if let Expr::New { callee, arguments } = expr {
            self.analyze_expr(callee);
            for a in arguments {
                self.analyze_expr(a);
            }
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
        if let Expr::Seq { left, right } = expr {
            self.analyze_expr(left);
            self.analyze_expr(right);
        }
        if let Expr::Spread { arg } = expr {
            self.analyze_expr(arg);
        }
    }

    fn analyze_call(&mut self, callee: &Expr, args: &[Expr]) {
        // Track which variables are passed as mutable references
        // For now, mark all variables used in calls as borrowed
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
            match &m.prop {
                super::ObjectProp::Init { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Get { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Set { value, .. } => {
                    self.analyze_expr(value);
                }
                super::ObjectProp::Method { value, .. } => self.analyze_expr(value),
                super::ObjectProp::Spread { arg } => self.analyze_expr(arg),
            }
        }
    }

    /// Analyze an assignment and determine if it's a mutation
    fn analyze_assign(&mut self, op: &super::AssignOp, left: &Expr, right: &Expr) {
        // First analyze the right-hand side
        self.analyze_expr(right);

        // Check if this is a mutation
        if self.is_compound_assign(op) || self.is_member_mutation(left) {
            self.mark_as_mutated(left);
        } else if self.is_simple_assign(op) {
            // Simple assignment like `x = y` creates an alias
            if let Expr::Ident { name: left_name } = left {
                self.analyze_expr(right);
                // If right is an identifier, create an alias
                if let Expr::Ident { name: right_name } = right {
                    self.aliases
                        .entry(left_name.clone())
                        .or_default()
                        .insert(right_name.clone());
                }
            }
        }
    }

    fn is_simple_assign(&self, op: &super::AssignOp) -> bool {
        matches!(op, super::AssignOp::Assign)
    }

    fn is_compound_assign(&self, op: &super::AssignOp) -> bool {
        use super::AssignOp as A;
        matches!(
            op,
            A::AddAssign | A::SubAssign | A::MulAssign | A::DivAssign | A::ModAssign
                | A::BitXorAssign | A::BitAndAssign | A::BitOrAssign
                | A::ShlAssign | A::ShrAssign | A::UShrAssign
        )
    }

    fn is_member_mutation(&self, left: &Expr) -> bool {
        matches!(left, Expr::Member { .. } | Expr::StaticMember { .. })
    }

    fn mark_as_mutated(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident { name } => self.mark_ident_mutated(name),
            Expr::Member { .. } | Expr::StaticMember { .. } => self.mark_member_mutated(expr),
            _ => {}
        }
    }

    fn mark_ident_mutated(&mut self, name: &str) {
        self.mut_vars.insert(name.to_string());
        if let Some(aliases) = self.aliases.get(name) {
            for alias in aliases {
                self.mut_vars.insert(alias.clone());
            }
        }
        for (key, alias_set) in &self.aliases {
            if alias_set.contains(name) {
                self.mut_vars.insert(key.clone());
            }
        }
    }

    fn mark_member_mutated(&mut self, expr: &Expr) {
        match expr {
            Expr::Member { obj, property, .. } => {
                self.mark_as_mutated(obj);
                let prop_name = self.extract_property_name(property);
                if let Expr::Ident { name } = &**obj {
                    let nested_key = format!("{}.{}", name, prop_name);
                    self.mut_vars.insert(nested_key);
                }
            }
            Expr::StaticMember { obj, property, .. } => {
                self.mark_as_mutated(obj);
                if let Expr::Ident { name } = &**obj {
                    let nested_key = format!("{}.{}", name, property);
                    self.mut_vars.insert(nested_key);
                }
            }
            _ => {}
        }
    }

    fn extract_property_name(&self, property: &Expr) -> String {
        match property {
            Expr::Ident { name } => name.clone(),
            Expr::String(s) => s.clone(),
            Expr::Number(n) => n.to_string(),
            _ => "".to_string(),
        }
    }

    /// Track that a variable may be aliased to another
    fn track_alias(&mut self, to: &str, from: &str) {
        self.aliases
            .entry(to.to_string())
            .or_default()
            .insert(from.to_string());
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

/// Infer ownership annotations for a class method
/// For now, we conservatively assume all parameters are Owned
/// since analyzing arbitrary expressions is complex
#[allow(dead_code)]
pub fn infer_method_ownership(method: &mut ClassMethod) {
    // For now, conservatively assume all params are Owned
    for param in &mut method.params {
        param.ownership = Ownership::Owned;
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
