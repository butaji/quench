//! # Type Inference
//!
//! Infers Rust types from TypeScript expressions.
//! Handles literals, operators, and composite types.

use swc_ecma_ast::*;
use super::{TypeInfo, TypeMap, StructInfo, EnumInfo, EnumVariant, FunctionInfo};
use super::context::AnalysisContext;

/// Infers types from TypeScript AST.
#[derive(Debug)]
pub struct TypeInferrer {
    /// Inferred types for this file
    types: TypeMap,
}

impl TypeInferrer {
    /// Create a new type inferrer.
    pub fn new() -> Self {
        Self { types: TypeMap::default() }
    }

    /// Infer all types in a module.
    pub fn infer_types(&mut self, module: &Module, ctx: &AnalysisContext) -> crate::Result<TypeMap> {
        for item in &module.body {
            self.infer_module_item(item, ctx)?;
        }
        Ok(std::mem::take(&mut self.types))
    }

    /// Infer type for a module item.
    fn infer_module_item(&mut self, item: &ModuleItem, ctx: &AnalysisContext) -> crate::Result<()> {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => self.infer_decl(decl, ctx),
            _ => Ok(()),
        }
    }

    /// Infer type for a declaration.
    fn infer_decl(&mut self, decl: &Decl, ctx: &AnalysisContext) -> crate::Result<()> {
        match decl {
            Decl::Fn(f) => {
                if f.ident.sym != *"_" {
                    let info = self.infer_function_type(&f.function, ctx);
                    self.types.insert(f.ident.sym.to_string(), info);
                }
            }
            Decl::Var(v) => {
                for declarator in &v.decls {
                    self.infer_var_declarator(declarator, ctx)?;
                }
            }
            Decl::TsTypeAlias(t) => {
                let info = self.infer_ts_type(&t.type_ann, ctx);
                self.types.insert(t.id.sym.to_string(), info);
            }
            Decl::TsEnum(e) => {
                let info = self.infer_enum(e, ctx);
                self.types.insert(e.id.sym.to_string(), info);
            }
            _ => {}
        }
        Ok(())
    }

    /// Infer a variable declarator.
    fn infer_var_declarator(&mut self, declarator: &VarDeclarator, ctx: &AnalysisContext) -> crate::Result<()> {
        let name = match &declarator.name {
            Pat::Ident(ident) => ident.id.sym.to_string(),
            Pat::Object(_) | Pat::Array(_) => return Ok(()), // Destructuring - infer each part
            _ => return Ok(()),
        };

        let type_info = match &declarator.init {
            Some(init) => self.infer_expr(init, ctx)?,
            None => {
                declarator.name.as_ident().and_then(|i| i.type_ann.as_ref())
                    .map(|t| self.infer_ts_type(&t.type_ann, ctx))
                    .unwrap_or(TypeInfo::Unknown)
            }
        };

        // Check for explicit type annotation
        if let Some(Pat::Ident(ident)) = declarator.name.as_ref() {
            if let Some(type_ann) = &ident.type_ann {
                self.types.insert(name, self.infer_ts_type(&type_ann.type_ann, ctx));
                return Ok(());
            }
        }

        self.types.insert(name, type_info);
        Ok(())
    }

    /// Infer type from an expression.
    fn infer_expr(&mut self, expr: &Expr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        match expr {
            Expr::Lit(lit) => self.infer_lit(lit),
            Expr::Ident(i) => {
                self.types.get(&i.sym.to_string()).cloned().unwrap_or(TypeInfo::Unknown)
            }
            Expr::Array(a) => {
                let elem_type = a.elems.first()
                    .map(|e| self.infer_expr(&e.as_ref().unwrap().expr, ctx))
                    .unwrap_or(Ok(TypeInfo::Unknown))?;
                Ok(TypeInfo::Array(Box::new(elem_type)))
            }
            Expr::Object(o) => {
                let fields: Vec<(String, TypeInfo)> = o.props.iter().filter_map(|p| {
                    let key = match p {
                        PropOrSpread::Prop(Prop::KeyValue(kv)) => {
                            let name = match &kv.key {
                                PropName::Str(s) => s.value.to_string(),
                                PropName::Ident(i) => i.sym.to_string(),
                                PropName::Num(n) => n.value.to_string(),
                                _ => return None,
                            };
                            let value_type = self.infer_prop(&Prop::KeyValue(kv.clone()), ctx).ok()?;
                            Some((name, value_type))
                        }
                        _ => None,
                    };
                    key
                }).collect();
                Ok(TypeInfo::Struct(StructInfo {
                    name: String::new(),
                    fields,
                }))
            }
            Expr::Bin(b) => self.infer_bin_expr(b, ctx),
            Expr::Unary(u) => self.infer_expr(&u.arg, ctx),
            Expr::Call(c) => self.infer_call(c, ctx),
            Expr::Arrow(f) => self.infer_arrow_type(f, ctx),
            Expr::Fn(f) => Ok(self.infer_function_type(&f.function, ctx)),
            Expr::Cond(c) => {
                let cons = self.infer_expr(&c.cons, ctx)?;
                let alt = self.infer_expr(&c.alt, ctx)?;
                Ok(cons) // Union of both branches
            }
            Expr::Member(m) => {
                if m.computed {
                    return Ok(TypeInfo::Unknown);
                }
                // Accessing struct field - need to look up the struct type
                self.infer_member_access(m, ctx)
            }
            Expr::Paren(p) => self.infer_expr(&p.expr, ctx),
            Expr::Tpl(t) => {
                // Template with expressions - returns string
                for expr in &t.exprs {
                    self.infer_expr(expr, ctx)?;
                }
                Ok(TypeInfo::String)
            }
            Expr::Seq(s) => {
                s.exprs.last()
                    .map(|e| self.infer_expr(e, ctx))
                    .unwrap_or(Ok(TypeInfo::Unknown))
            }
            Expr::Assign(a) => self.infer_expr(&a.value, ctx),
            Expr::Await(a) => {
                if let TypeInfo::Function(f) = self.infer_expr(&a.arg, ctx)? {
                    Ok(*f.return_type)
                } else {
                    Ok(TypeInfo::Unknown)
                }
            }
            Expr::Update(_) => Ok(TypeInfo::Float), // Post/pre increment
            Expr::TsTypeAssertion(t) => self.infer_ts_type(&t.type_ann, ctx),
            Expr::TsAs(t) => self.infer_ts_type(&t.type_ann, ctx),
            _ => Ok(TypeInfo::Unknown),
        }
    }

    /// Infer type from a literal.
    fn infer_lit(&self, lit: &Lit) -> TypeInfo {
        match lit {
            Lit::Num(n) => {
                // Check if it's an integer literal
                if n.value.fract() == 0.0 && n.value.abs() <= i32::MAX as f64 {
                    TypeInfo::Integer(n.value as i64)
                } else {
                    TypeInfo::Float
                }
            }
            Lit::Str(s) => TypeInfo::StringLiteral(s.value.to_string()),
            Lit::Bool(_) => TypeInfo::Boolean,
            Lit::Null(_) => TypeInfo::Unknown, // Null by itself is unknown
            Lit::BigInt(_) => TypeInfo::Integer(0), // i64 representation
            _ => TypeInfo::Unknown,
        }
    }

    /// Infer type from a binary expression.
    fn infer_bin_expr(&mut self, bin: &BinExpr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        // Check if this is a Result pattern
        if let Expr::Object(obj) = &*bin.right {
            if let Some(result_type) = self.check_result_pattern(obj, ctx)? {
                return Ok(result_type);
            }
        }

        match bin.op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                let left = self.infer_expr(&bin.left, ctx)?;
                let right = self.infer_expr(&bin.right, ctx)?;
                
                // If either is Float, result is Float
                if matches!(left, TypeInfo::Float) || matches!(right, TypeInfo::Float) {
                    return Ok(TypeInfo::Float);
                }
                // If both are integers, result is integer
                if matches!(&left, TypeInfo::Integer(_)) && matches!(&right, TypeInfo::Integer(_)) {
                    return Ok(TypeInfo::Integer(0)); // Generic integer type
                }
                // String concatenation
                if matches!(left, TypeInfo::String | TypeInfo::StringLiteral(_))
                    || matches!(right, TypeInfo::String | TypeInfo::StringLiteral(_)) {
                    return Ok(TypeInfo::String);
                }
                Ok(TypeInfo::Float)
            }
            BinaryOp::EqEq | BinaryOp::NotEq
            | BinaryOp::EqEqEq | BinaryOp::NotEqEq
            | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
            | BinaryOp::LogicalAnd | BinaryOp::LogicalOr
            | BinaryOp::BinAnd | BinaryOp::BinOr | BinaryOp::BinXor
            | BinaryOp::LShift | BinaryOp::RShift | BinaryOp::ZeroFillRShift => {
                Ok(TypeInfo::Boolean)
            }
            BinaryOp::Mod | BinaryOp::Exp | BinaryOp::NullishCoalescing => {
                Ok(TypeInfo::Float)
            }
        }
    }

    /// Check if an object pattern matches Result<T, E>.
    fn check_result_pattern(&mut self, obj: &ObjectExpr, ctx: &AnalysisContext) -> crate::Result<Option<TypeInfo>> {
        let mut has_ok = false;
        let mut has_error = false;
        let mut ok_type = TypeInfo::Unknown;
        let mut error_type = TypeInfo::Unknown;

        for prop in &obj.props {
            if let PropOrSpread::Prop(Prop::KeyValue(kv)) = prop {
                let key_name = match &kv.key {
                    PropName::Str(s) => s.value.to_string(),
                    PropName::Ident(i) => i.sym.to_string(),
                    _ => continue,
                };

                match key_name.as_str() {
                    "ok" => {
                        has_ok = true;
                        if let Expr::Member(m) = &kv.value {
                            if let Expr::Lit(Lit::True(_)) = &*m.obj {
                                ok_type = self.infer_expr(&m.prop, ctx)?;
                            }
                        } else {
                            ok_type = self.infer_expr(&kv.value, ctx)?;
                        }
                    }
                    "error" => {
                        has_error = true;
                        error_type = self.infer_expr(&kv.value, ctx)?;
                    }
                    "value" => {
                        has_ok = true;
                        ok_type = self.infer_expr(&kv.value, ctx)?;
                    }
                    _ => {}
                }
            }
        }

        if has_ok || has_error {
            Ok(Some(TypeInfo::Result(Box::new(ok_type), Box::new(error_type))))
        } else {
            Ok(None)
        }
    }

    /// Infer type from a function call.
    fn infer_call(&mut self, call: &CallExpr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        // Check if it's a known function
        if let Expr::Ident(ident) = &*call.callee {
            if let Some(func_type) = self.types.get(&ident.sym.to_string()) {
                if let TypeInfo::Function(f) = func_type {
                    return Ok(*f.return_type.clone());
                }
            }
        }

        // Check for native imports
        if let Expr::Member(m) = &*call.callee {
            if let Expr::Ident(ident) = &*m.obj {
                if ident.sym == *"native" {
                    // Native function - return type unknown
                    return Ok(TypeInfo::Unknown);
                }
            }
        }

        // Generic unknown call
        Ok(TypeInfo::Unknown)
    }

    /// Infer arrow function type.
    fn infer_arrow_type(&mut self, arrow: &ArrowExpr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        let params: Vec<(String, TypeInfo)> = arrow.params.iter().map(|p| {
            let name = match p.pat.as_ident() {
                Some(i) => i.id.sym.to_string(),
                None => "_".to_string(),
            };
            let type_info = p.pat.as_ident()
                .and_then(|i| i.type_ann.as_ref())
                .map(|t| self.infer_ts_type(&t.type_ann, ctx))
                .unwrap_or(TypeInfo::Unknown);
            (name, type_info)
        }).collect();

        let return_type = match &arrow.body {
            BlockStmtOrExpr::BlockStmt(b) => {
                // Look for return statements
                let mut ret_type = TypeInfo::Unknown;
                for stmt in &b.stmts {
                    if let Stmt::Return(r) = stmt {
                        if let Some(expr) = &r.value {
                            ret_type = self.infer_expr(expr, ctx)?;
                            break;
                        }
                    }
                }
                ret_type
            }
            BlockStmtOrExpr::Expr(e) => self.infer_expr(e, ctx)?,
        };

        Ok(TypeInfo::Function(FunctionInfo {
            params,
            return_type,
            is_async: arrow.is_async,
        }))
    }

    /// Infer function type from Function.
    fn infer_function_type(&mut self, func: &Function, ctx: &AnalysisContext) -> TypeInfo {
        let params: Vec<(String, TypeInfo)> = func.params.iter().map(|p| {
            let name = p.pat.as_ident()
                .map(|i| i.id.sym.to_string())
                .unwrap_or_else(|| "_".to_string());
            let type_info = p.pat.as_ident()
                .and_then(|i| i.type_ann.as_ref())
                .map(|t| self.infer_ts_type(&t.type_ann, ctx))
                .unwrap_or(TypeInfo::Unknown);
            (name, type_info)
        }).collect();

        let return_type = func.return_type.as_ref()
            .map(|t| self.infer_ts_type(&t.type_ann, ctx))
            .unwrap_or(TypeInfo::Unknown);

        TypeInfo::Function(FunctionInfo {
            params,
            return_type,
            is_async: func.is_async,
        })
    }

    /// Infer member access type.
    fn infer_member_access(&mut self, member: &MemberExpr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        let obj_type = self.infer_expr(&member.obj, ctx)?;
        
        if let TypeInfo::Struct(s) = obj_type {
            let field_name = match &member.prop {
                Expr::Ident(i) => i.sym.to_string(),
                _ => return Ok(TypeInfo::Unknown),
            };
            Ok(s.fields.iter()
                .find(|(n, _)| n == &field_name)
                .map(|(_, t)| t.clone())
                .unwrap_or(TypeInfo::Unknown))
        } else {
            Ok(TypeInfo::Unknown)
        }
    }

    /// Infer type from a property.
    fn infer_prop(&self, prop: &Prop, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        match prop {
            Prop::KeyValue(kv) => {
                let mut temp_inferrer = TypeInferrer::default();
                temp_inferrer.infer_expr(&kv.value, ctx)
            }
            _ => Ok(TypeInfo::Unknown),
        }
    }

    /// Infer type from TypeScript type annotation.
    fn infer_ts_type(&self, ts_type: &TsType, ctx: &AnalysisContext) -> TypeInfo {
        match ts_type {
            TsType::TsKeywordType(k) => match k.kind {
                TsKeywordTypeKind::TsNumberKeyword => TypeInfo::Float,
                TsKeywordTypeKind::TsStringKeyword => TypeInfo::String,
                TsKeywordTypeKind::TsBooleanKeyword => TypeInfo::Boolean,
                TsKeywordTypeKind::TsNullKeyword => TypeInfo::Unknown,
                TsKeywordTypeKind::TsUndefinedKeyword => TypeInfo::Unknown,
                TsKeywordTypeKind::TsVoidKeyword => TypeInfo::Unknown,
                TsKeywordTypeKind::TsAnyType | TsKeywordTypeKind::TsUnknownType => TypeInfo::Unknown,
                _ => TypeInfo::Unknown,
            },
            TsType::TsArrayType(a) => {
                TypeInfo::Array(Box::new(self.infer_ts_type(&a.elem_type, ctx)))
            }
            TsType::TsUnionOrIntersectionType(t) => {
                if t.ts_type_union.is_some() {
                    // Union type - could be Option or Result
                    TypeInfo::Unknown
                } else {
                    TypeInfo::Unknown
                }
            }
            TsType::TsTypeRef(t) => {
                let name = t.type_name.as_str();
                match name {
                    "Array" | "Vec" => {
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                let inner = self.infer_ts_type(&params.params[0], ctx);
                                return TypeInfo::Array(Box::new(inner));
                            }
                        }
                        TypeInfo::Array(Box::new(TypeInfo::Unknown))
                    }
                    "Option" => {
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                let inner = self.infer_ts_type(&params.params[0], ctx);
                                return TypeInfo::Option(Box::new(inner));
                            }
                        }
                        TypeInfo::Option(Box::new(TypeInfo::Unknown))
                    }
                    "Result" => {
                        if let Some(params) = &t.type_params {
                            if params.params.len() >= 2 {
                                let ok = self.infer_ts_type(&params.params[0], ctx);
                                let err = self.infer_ts_type(&params.params[1], ctx);
                                return TypeInfo::Result(Box::new(ok), Box::new(err));
                            }
                        }
                        TypeInfo::Result(Box::new(TypeInfo::Unknown), Box::new(TypeInfo::Unknown))
                    }
                    _ => TypeInfo::Unknown,
                }
            }
            TsType::TsLiteralType(l) => match &l.lit {
                TsLit::Str(s) => TypeInfo::StringLiteral(s.value.to_string()),
                TsLit::Num(n) => {
                    if n.value.fract() == 0.0 {
                        TypeInfo::Integer(n.value as i64)
                    } else {
                        TypeInfo::Float
                    }
                }
                TsLit::BigInt(b) => TypeInfo::Integer(0),
                TsLit::Boolean(b) => TypeInfo::Boolean,
            },
            TsType::TsTupleType(t) => {
                let types: Vec<TypeInfo> = t.elem_types.iter()
                    .map(|e| self.infer_ts_type(&e.ty, ctx))
                    .collect();
                TypeInfo::Struct(StructInfo {
                    name: String::new(),
                    fields: types.into_iter().enumerate()
                        .map(|(i, t)| (format!("_{}", i), t))
                        .collect(),
                })
            }
            TsType::TsParenthesizedType(p) => self.infer_ts_type(&p.type_ann, ctx),
            _ => TypeInfo::Unknown,
        }
    }

    /// Infer enum type.
    fn infer_enum(&self, e: &TsEnumDecl, _ctx: &AnalysisContext) -> TypeInfo {
        let variants: Vec<EnumVariant> = e.members.iter().map(|m| {
            let tag = match &m.id {
                TsEnumMemberId::Str(s) => s.value.to_string(),
                TsEnumMemberId::Computed(_) => String::new(),
            };
            let fields: Vec<(String, TypeInfo)> = m.init.as_ref().map(|init| {
                // Could parse the init expression to get field types
                (tag.clone(), TypeInfo::Unknown)
            }).into_iter().collect();
            EnumVariant { tag, fields }
        }).collect();

        TypeInfo::Enum(EnumInfo {
            name: e.id.sym.to_string(),
            variants,
        })
    }
}

impl Default for TypeInferrer {
    fn default() -> Self {
        Self::new()
    }
}
