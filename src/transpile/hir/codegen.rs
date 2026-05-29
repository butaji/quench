//! Rust code generation from HIR
//!
//! Designed to meet linter constraints: file ≤500 lines, fn ≤40 lines, complexity ≤10

use super::{Expr, FunctionDecl, Stmt, Type, TypeGen};
use std::collections::HashMap;

/// Code generation context
pub struct Codegen {
    structs: HashMap<String, StructDef>,
    depth: usize,
    type_gen: TypeGen,
}

#[derive(Clone)]
struct StructDef {
    name: String,
    fields: Vec<(String, Type)>,
}

impl Codegen {
    /// Generate Rust module from HIR statements
    pub fn gen_module(&mut self, stmts: &[Stmt]) -> String {
        let mut out = String::from("use serde::{Deserialize, Serialize};\n\n");
        for stmt in stmts {
            self.collect_structs(stmt);
        }
        for def in self.structs.values() {
            out.push_str(&self.gen_struct(def));
            out.push('\n');
        }
        for stmt in stmts {
            out.push_str(&self.gen_stmt(stmt));
        }
        out
    }

    fn collect_structs(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr { expr } => self.collect_expr(expr),
            Stmt::FunctionDecl(f) => {
                if let Some(body) = &f.body {
                    for s in &body.0 {
                        self.collect_structs(s);
                    }
                }
            }
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.collect_structs(s);
                }
            }
            _ => {}
        }
    }

    fn collect_expr(&mut self, expr: &Expr) {
        if let Expr::Object { members } = expr {
            let fields = self.infer_object_fields(members);
            if !fields.is_empty() {
                let name = format!("Anon_{}", self.structs.len());
                self.structs
                    .insert(name.clone(), StructDef { name, fields });
            }
        }
    }

    fn infer_object_fields(&self, members: &[super::ObjectMemberExpr]) -> Vec<(String, Type)> {
        let mut fields = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in members {
            if let super::ObjectProp::Init { key, value, .. } = &m.prop {
                let k = self.key_str(key);
                if seen.contains(&k) {
                    fields.retain(|(n, _)| n != &k);
                }
                seen.insert(k.clone());
                fields.push((k, self.infer_type(value)));
            }
        }
        fields
    }

    fn key_str(&self, key: &super::PropKey) -> String {
        match key {
            super::PropKey::Str(s) => s.clone(),
            super::PropKey::Num(n) => n.to_string(),
            super::PropKey::Computed { .. } => String::new(),
        }
    }

    fn infer_type(&self, expr: &Expr) -> Type {
        use super::Expr as E;
        match expr {
            E::String(_) => Type::String,
            E::Number(_) => Type::Number,
            E::Boolean(_) => Type::Boolean,
            E::Null => Type::Null,
            E::Undefined => Type::Undefined,
            E::BigInt(_) => Type::BigInt,
            _ => self.infer_type_complex(expr),
        }
    }

    fn infer_type_complex(&self, expr: &Expr) -> Type {
        use super::Expr as E;
        match expr {
            E::Array { elems } => self.infer_array_type(elems),
            E::Object { members } => self.infer_object_type(members),
            _ => Type::Unknown,
        }
    }

    fn infer_object_type(&self, members: &[super::ObjectMemberExpr]) -> Type {
        let fields = self.infer_object_fields(members);
        Type::Object {
            members: fields
                .into_iter()
                .map(|(k, t)| super::TypeMember {
                    key: k,
                    type_: t,
                    optional: false,
                    readonly: false,
                })
                .collect(),
        }
    }

    fn infer_array_type(&self, elems: &[Option<Expr>]) -> Type {
        let types: Vec<Type> = elems
            .iter()
            .filter_map(|e| e.as_ref().map(|e| self.infer_type(e)))
            .collect();
        if types.is_empty() {
            Type::Any
        } else {
            Type::Array {
                elem: Box::new(types.into_iter().next().unwrap_or(Type::Any)),
            }
        }
    }

    fn gen_struct(&self, def: &StructDef) -> String {
        let mut out = format!(
            "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n",
            def.name
        );
        for (f, t) in &def.fields {
            out.push_str(&format!("    pub {}: {},\n", f, self.type_gen.gen_type(t)));
        }
        out.push_str("}\n");
        out
    }

    fn gen_stmt(&mut self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Expr { expr } => self.gen_expr(expr),
            Stmt::FunctionDecl(f) => self.gen_fn_decl(f),
            Stmt::Block(stmts) => self.gen_block(stmts),
            Stmt::Return { arg } => self.gen_return(arg),
            Stmt::If {
                test,
                consequent,
                alternate,
            } => self.gen_if(test, consequent, alternate),
            Stmt::While { test, body } => self.gen_while(test, body),
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.gen_for(init, test, update, body),
            _ => String::new(),
        }
    }

    fn gen_block(&mut self, stmts: &[Stmt]) -> String {
        let mut out = String::from("{\n");
        for s in stmts {
            out.push_str("    ");
            out.push_str(&self.gen_stmt(s).replace('\n', "\n    "));
        }
        out.push_str("}\n");
        out
    }

    fn gen_return(&mut self, arg: &Option<Expr>) -> String {
        match arg {
            Some(e) => format!("return {};\n", self.gen_expr(e)),
            None => "return;\n".to_string(),
        }
    }

    fn gen_if(&mut self, test: &Expr, cons: &Box<Stmt>, alt: &Option<Box<Stmt>>) -> String {
        let t = self.gen_expr(test);
        let c = self.gen_stmt(cons);
        let mut out = format!("if {} {{\n{}\n}}", t, indent(&c));
        if let Some(a) = alt {
            let ac = self.gen_stmt(a);
            out.push_str(&format!(" else {{\n{}\n}}", indent(&ac)));
        }
        out.push('\n');
        out
    }

    fn gen_while(&mut self, test: &Expr, body: &Box<Stmt>) -> String {
        let t = self.gen_expr(test);
        let b = self.gen_stmt(body);
        format!("while {} {{\n{}\n}}\n", t, indent(&b))
    }

    fn gen_for(
        &mut self,
        init: &Option<super::ForInit>,
        test: &Option<Expr>,
        update: &Option<Expr>,
        body: &Box<Stmt>,
    ) -> String {
        let i = self.gen_for_init(init);
        let t = self.gen_opt_expr(test);
        let u = self.gen_opt_expr(update);
        let b = self.gen_stmt(body);
        format!("for {}; {}; {} {{\n{}\n}}\n", i, t, u, indent(&b))
    }

    fn gen_for_init(&mut self, init: &Option<super::ForInit>) -> String {
        match init {
            Some(super::ForInit::Variable(_, decls)) => {
                let parts: Vec<String> = decls
                    .iter()
                    .filter_map(|(n, i)| i.as_ref().map(|e| format!("{}: {}", n, self.gen_expr(e))))
                    .collect();
                format!("let {}", parts.join(", "))
            }
            Some(super::ForInit::Expr(e)) => self.gen_expr(e),
            None => String::new(),
        }
    }

    fn gen_opt_expr(&mut self, expr: &Option<Expr>) -> String {
        expr.as_ref().map(|e| self.gen_expr(e)).unwrap_or_default()
    }

    fn gen_fn_decl(&mut self, func: &FunctionDecl) -> String {
        let params = self.gen_params(&func.params);
        let ret = self.gen_ret_type(&func.return_type, func.throws, &func.error_type);
        let body = self.gen_fn_body(&func.body);
        let a = if func.is_async { "async " } else { "" };
        format!(
            "pub {}fn {}({}){} {{\n{}\n}}\n",
            a,
            func.name,
            params,
            ret,
            indent(&body)
        )
    }
    
    fn gen_params(&mut self, params: &[super::Param]) -> String {
        params
            .iter()
            .map(|p| self.gen_param(p))
            .collect::<Vec<_>>()
            .join(", ")
    }
    
    fn gen_param(&self, param: &super::Param) -> String {
        let name = &param.name;
        let ty = param
            .type_
            .as_ref()
            .map(|x| format!(": {}", self.type_gen.gen_type(x)))
            .unwrap_or_default();
        match param.ownership {
            super::Ownership::Owned => format!("{}{}", name, ty),
            super::Ownership::Borrow => format!("&{}", name),
            super::Ownership::Mut => format!("&mut {}", name),
        }
    }
    
    fn gen_ret_type(&mut self, ret: &Option<Type>, throws: bool, error_type: &Option<Type>) -> String {
        let base_ret = ret.as_ref().map(|t| self.type_gen.gen_type(t)).unwrap_or_else(|| "()".to_string());
        if throws {
            let err_type = error_type.as_ref().map(|t| self.type_gen.gen_type(t)).unwrap_or_else(|| "JsValue".to_string());
            format!(" -> Result<{}, {}>", base_ret, err_type)
        } else {
            format!(" -> {}", base_ret)
        }
    }

    fn gen_fn_body(&mut self, body: &Option<super::Block>) -> String {
        match body {
            Some(b) => {
                let mut out = String::new();
                for s in &b.0 {
                    out.push_str(&self.gen_stmt(s));
                }
                out
            }
            None => "unimplemented!()\n".to_string(),
        }
    }

    // Expression generation - split by type category

    fn gen_expr(&mut self, expr: &Expr) -> String {
        use super::Expr as E;
        match expr {
            E::Number(n) => n.to_string(),
            E::Boolean(b) => b.to_string(),
            E::Null => "null".to_string(),
            E::Undefined => "None".to_string(),
            E::Ident { name } => name.clone(),
            _ => self.gen_expr_b(expr),
        }
    }

    fn gen_expr_b(&mut self, expr: &Expr) -> String {
        use super::Expr as E;
        match expr {
            E::String(s) => self.gen_string(s),
            E::BigInt(n) => format!("{}i64", n),
            E::Array { elems } => self.gen_array(elems),
            E::Object { members } => self.gen_object(members),
            E::Bin { op, left, right } => self.gen_bin(op, left, right),
            _ => self.gen_expr_c(expr),
        }
    }

    fn gen_expr_c(&mut self, expr: &Expr) -> String {
        use super::Expr as E;
        match expr {
            E::Cond {
                test,
                consequent,
                alternate,
            } => self.gen_cond(test, consequent, alternate),
            E::Call { callee, arguments } => self.gen_call(callee, arguments),
            E::Member {
                obj,
                property,
                computed,
            } => self.gen_member(obj, property, *computed),
            E::StaticMember { obj, property } => self.gen_static(obj, property),
            _ => self.gen_expr_d(expr),
        }
    }

    fn gen_expr_d(&mut self, expr: &Expr) -> String {
        use super::Expr as E;
        match expr {
            E::Assign { op, left, right } => self.gen_assign(op, left, right),
            E::Unary { op, arg, .. } => self.gen_unary(op, arg),
            E::ArrowFunction { params, body, .. } => self.gen_arrow(params, body),
            E::Function(func) => self.gen_fn(func),
            _ => "/* unimplemented */".to_string(),
        }
    }

    fn gen_string(&self, s: &str) -> String {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    }

    fn gen_array(&mut self, elems: &[Option<Expr>]) -> String {
        let inner: Vec<String> = elems
            .iter()
            .map(|e| {
                e.as_ref()
                    .map(|x| self.gen_expr(x))
                    .unwrap_or_else(|| "Value::Undefined".to_string())
            })
            .collect();
        format!("vec![{}]", inner.join(", "))
    }

    fn gen_object(&mut self, members: &[super::ObjectMemberExpr]) -> String {
        let fields = self.infer_object_fields(members);
        if fields.is_empty() {
            return "serde_json::json!({})".to_string();
        }
        let name = self.get_struct_for_fields(&fields);
        let vals: Vec<String> = fields
            .iter()
            .filter_map(|(k, _)| {
                members
                    .iter()
                    .find(|m| {
                        if let super::ObjectProp::Init {
                            key: super::PropKey::Str(s),
                            ..
                        } = &m.prop
                        {
                            s == k
                        } else {
                            false
                        }
                    })
                    .and_then(|m| {
                        if let super::ObjectProp::Init { value, .. } = &m.prop {
                            Some(format!("{}: {}", k, self.gen_expr(value)))
                        } else {
                            None
                        }
                    })
            })
            .collect();
        format!("{} {{ {} }}", name, vals.join(", "))
    }

    fn get_struct_for_fields(&self, fields: &[(String, Type)]) -> String {
        for (name, def) in &self.structs {
            if &def.fields == fields {
                return name.clone();
            }
        }
        "serde_json::json!({})".to_string()
    }

    fn gen_bin(&mut self, op: &super::BinaryOp, left: &Expr, right: &Expr) -> String {
        let l = self.gen_expr(left);
        let r = self.gen_expr(right);
        let o = self.bin_op_str(op);
        format!("({} {} {})", l, o, r)
    }

    fn bin_op_str(&self, op: &super::BinaryOp) -> &'static str {
        use super::BinaryOp as B;
        match op {
            B::Add => "+",
            B::Sub => "-",
            B::Mul => "*",
            B::Div => "/",
            B::Mod => "%",
            B::Eq | B::StrictEq => "==",
            B::Neq | B::StrictNeq => "!=",
            B::Lt => "<",
            B::Lte => "<=",
            B::Gt => ">",
            B::Gte => ">=",
            B::LogicalAnd => "&&",
            B::LogicalOr => "||",
            _ => "/* op */",
        }
    }

    fn gen_cond(&mut self, test: &Expr, cons: &Expr, alt: &Expr) -> String {
        let t = self.gen_expr(test);
        let c = self.gen_expr(cons);
        let a = self.gen_expr(alt);
        format!("if {} {{ {} }} else {{ {} }}", t, c, a)
    }

    fn gen_call(&mut self, callee: &Expr, args: &[Expr]) -> String {
        let c = self.gen_expr(callee);
        let a: Vec<String> = args.iter().map(|x| self.gen_expr(x)).collect();
        format!("{}({})", c, a.join(", "))
    }

    fn gen_member(&mut self, obj: &Expr, prop: &Expr, computed: bool) -> String {
        let o = self.gen_expr(obj);
        let p = self.gen_expr(prop);
        if computed {
            format!("{}[{}]", o, p)
        } else {
            format!("{}.{}", o, p)
        }
    }

    fn gen_static(&mut self, obj: &Expr, property: &str) -> String {
        format!("{}.{}", self.gen_expr(obj), property)
    }

    fn gen_assign(&mut self, op: &super::AssignOp, left: &Expr, right: &Expr) -> String {
        let l = self.gen_expr(left);
        let r = self.gen_expr(right);
        let o = match op {
            super::AssignOp::Assign => "=",
            super::AssignOp::AddAssign => "+=",
            _ => "=",
        };
        format!("{} {} {}", l, o, r)
    }

    fn gen_unary(&mut self, op: &super::UnaryOp, arg: &Expr) -> String {
        let a = self.gen_expr(arg);
        let o = match op {
            super::UnaryOp::Minus => "-",
            super::UnaryOp::Plus => "+",
            super::UnaryOp::Not => "!",
            super::UnaryOp::BitNot => "~",
            _ => "/* unary */",
        };
        format!("{}{}", o, a)
    }

    fn gen_arrow(&mut self, params: &[super::Param], body: &Expr) -> String {
        let p: Vec<String> = params.iter().map(|x| x.name.clone()).collect();
        format!("move |{}| {}", p.join(", "), self.gen_expr(body))
    }

    fn gen_fn(&mut self, func: &FunctionDecl) -> String {
        let p: Vec<String> = func.params.iter().map(|x| x.name.clone()).collect();
        match &func.body {
            Some(b) => {
                let mut out = String::new();
                for s in &b.0 {
                    out.push_str(&self.gen_stmt(s));
                }
                format!("|{}| {{\n{}\n}}", p.join(", "), indent(&out))
            }
            None => format!("|{}| unimplemented!()", p.join(", ")),
        }
    }

    // Type generation - split by category
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("    {}", l))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Default for Codegen {
    fn default() -> Self {
        Self {
            structs: HashMap::new(),
            depth: 0,
            type_gen: TypeGen::default(),
        }
    }
}
