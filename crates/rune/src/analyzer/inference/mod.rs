//! # Type Inference Module
//!
//! Infers Rust types from TypeScript AST nodes.

pub mod primitives;
pub mod ts_types;

pub use primitives::{infer_lit, infer_bin_expr_type, infer_bin_op_result};
pub use ts_types::infer_ts_type;

use swc_ecma_ast::*;
use crate::analyzer::{TypeInfo, TypeMap, StructInfo, EnumInfo, EnumVariant, FunctionInfo};
use crate::analyzer::context::AnalysisContext;

/// Main type inference engine.
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
                let info = infer_ts_type(&t.type_ann);
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
            _ => return Ok(()),
        };

        let type_info = match &declarator.init {
            Some(init) => self.infer_expr(init, ctx)?,
            None => declarator.name.as_ident().and_then(|i| i.type_ann.as_ref())
                .map(|t| infer_ts_type(&t.type_ann))
                .unwrap_or(TypeInfo::Unknown),
        };

        // Check for explicit type annotation
        if let Some(Pat::Ident(ident)) = declarator.name.as_ref() {
            if let Some(type_ann) = &ident.type_ann {
                self.types.insert(name, infer_ts_type(&type_ann.type_ann));
                return Ok(());
            }
        }

        self.types.insert(name, type_info);
        Ok(())
    }

    /// Infer type from an expression.
    fn infer_expr(&mut self, expr: &Expr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        let result = match expr {
            Expr::Lit(lit) => Ok(infer_lit(lit)),
            Expr::Ident(i) => {
                Ok(self.types.get(&i.sym.to_string()).cloned().unwrap_or(TypeInfo::Unknown))
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
            Expr::Bin(b) => Ok(self.infer_bin_expr(b, ctx)?),
            Expr::Unary(_) => Ok(TypeInfo::Float),
            Expr::Call(c) => self.infer_call(c, ctx),
            Expr::Arrow(a) => Ok(self.infer_arrow_type(a, ctx)?),
            Expr::Fn(f) => Ok(TypeInfo::Function(self.infer_function_type(&f.function, ctx))),
            Expr::Cond(c) => {
                let cons = self.infer_expr(&c.cons, ctx)?;
                Ok(cons)
            }
            Expr::Member(_) => Ok(TypeInfo::Unknown),
            Expr::Paren(p) => self.infer_expr(&p.expr, ctx),
            Expr::Tpl(_) => Ok(TypeInfo::String),
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
            Expr::Update(_) => Ok(TypeInfo::Float),
            Expr::TsTypeAssertion(t) => Ok(infer_ts_type(&t.type_ann)),
            Expr::TsAs(t) => Ok(infer_ts_type(&t.type_ann)),
            _ => Ok(TypeInfo::Unknown),
        };

        // Check for Result pattern
        if let Ok(TypeInfo::Unknown) = result {
            if let Expr::Object(obj) = expr {
                if let Some(result_type) = self.check_result_pattern(obj, ctx)? {
                    return Ok(result_type);
                }
            }
        }

        result
    }

    /// Infer type from a binary expression.
    fn infer_bin_expr(&mut self, bin: &BinExpr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        let left = self.infer_expr(&bin.left, ctx)?;
        let right = self.infer_expr(&bin.right, ctx)?;
        Ok(infer_bin_expr_type(&left, &right))
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
                        ok_type = self.infer_expr(&kv.value, ctx)?;
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
        if let Expr::Ident(ident) = &*call.callee {
            if let Some(func_type) = self.types.get(&ident.sym.to_string()) {
                if let TypeInfo::Function(f) = func_type {
                    return Ok(*f.return_type.clone());
                }
            }
        }
        Ok(TypeInfo::Unknown)
    }

    /// Infer arrow function type.
    fn infer_arrow_type(&mut self, arrow: &ArrowExpr, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        let params: Vec<(String, TypeInfo)> = arrow.params.iter().map(|p| {
            let name = p.pat.as_ident()
                .map(|i| i.id.sym.to_string())
                .unwrap_or_else(|| "_".to_string());
            let type_info = p.pat.as_ident()
                .and_then(|i| i.type_ann.as_ref())
                .map(|t| infer_ts_type(&t.type_ann))
                .unwrap_or(TypeInfo::Unknown);
            (name, type_info)
        }).collect();

        let return_type = match &arrow.body {
            BlockStmtOrExpr::BlockStmt(b) => {
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
                .map(|t| infer_ts_type(&t.type_ann))
                .unwrap_or(TypeInfo::Unknown);
            (name, type_info)
        }).collect();

        let return_type = func.return_type.as_ref()
            .map(|t| infer_ts_type(&t.type_ann))
            .unwrap_or(TypeInfo::Unknown);

        TypeInfo::Function(FunctionInfo {
            params,
            return_type,
            is_async: func.is_async,
        })
    }

    /// Infer type from a property.
    fn infer_prop(&self, prop: &Prop, ctx: &AnalysisContext) -> crate::Result<TypeInfo> {
        match prop {
            Prop::KeyValue(kv) => {
                // Simplified - would need full inference
                Ok(TypeInfo::Unknown)
            }
            _ => Ok(TypeInfo::Unknown),
        }
    }

    /// Infer enum type.
    fn infer_enum(&self, e: &TsEnumDecl, _ctx: &AnalysisContext) -> TypeInfo {
        let variants: Vec<EnumVariant> = e.members.iter().map(|m| {
            let tag = match &m.id {
                TsEnumMemberId::Str(s) => s.value.to_string(),
                TsEnumMemberId::Computed(_) => String::new(),
            };
            let fields: Vec<(String, TypeInfo)> = Vec::new();
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
