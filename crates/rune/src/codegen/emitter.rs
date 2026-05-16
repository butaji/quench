//! # Rust Emitter
//!
//! Core transpilation from TypeScript AST to Rust source.

use std::collections::HashMap;
use swc_ecma_ast::*;
use crate::{parser::SourceFile, analyzer::{AnalysisResult, BorrowMode}};
use super::{GeneratedModule, Import, ImportedName, CodegenOptions};
use super::emitters::{ExprEmitter, StmtEmitter};

/// Options for code emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// Generate source maps
    pub source_map: bool,
    /// Pretty print output
    pub pretty: bool,
}

/// Emits Rust code from TypeScript AST.
pub struct RustEmitter<'a> {
    source: &'a SourceFile,
    analysis: &'a AnalysisResult,
    imports: Vec<Import>,
    output: String,
    indent: usize,
    options: EmitOptions,
}

impl<'a> RustEmitter<'a> {
    /// Create a new emitter.
    pub fn new(source: &'a SourceFile, analysis: &'a AnalysisResult) -> Self {
        Self {
            source,
            analysis,
            imports: Vec::new(),
            output: String::new(),
            indent: 0,
            options: EmitOptions::default(),
        }
    }

    /// Emit the complete module.
    pub fn emit(mut self) -> crate::Result<GeneratedModule> {
        self.write_header()?;
        self.emit_module_items()?;
        self.write_footer()?;

        let name = self.source.path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module")
            .to_string();

        Ok(GeneratedModule {
            name,
            source: self.output,
            imports: self.imports,
        })
    }

    /// Write module header.
    fn write_header(&mut self) -> crate::Result<()> {
        self.push_line("use std::collections::HashMap;");
        self.push_line("use std::fmt::{self, Write};");
        self.push_line("");
        Ok(())
    }

    /// Write module footer.
    fn write_footer(&mut self) -> crate::Result<()> {
        Ok(())
    }

    /// Emit all module items.
    fn emit_module_items(&mut self) -> crate::Result<()> {
        for item in &self.source.module.body {
            self.emit_module_item(item)?;
            self.push_line("");
        }
        Ok(())
    }

    /// Emit a single module item.
    fn emit_module_item(&mut self, item: &ModuleItem) -> crate::Result<()> {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => self.emit_decl(decl),
            ModuleItem::ModuleDecl(decl) => self.emit_module_decl(decl),
            ModuleItem::Stmt(stmt) => {
                let mut emitter = StmtEmitter::new(self.analysis);
                emitter.emit_stmt(stmt);
                self.push(&emitter.into_output());
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Emit a declaration.
    fn emit_decl(&mut self, decl: &Decl) -> crate::Result<()> {
        match decl {
            Decl::Fn(f) => self.emit_fn(f),
            Decl::Var(v) => self.emit_var_decl(v),
            Decl::TsTypeAlias(t) => self.emit_type_alias(t),
            Decl::TsEnum(e) => self.emit_enum(e),
            Decl::TsInterface(i) => self.emit_interface(i),
            _ => Ok(()),
        }
    }

    /// Emit a function.
    fn emit_fn(&mut self, f: &FnDecl) -> crate::Result<()> {
        let name = self.mangle(&f.ident.sym.to_string());

        if f.function.is_async {
            self.push("pub async ");
        } else {
            self.push("pub ");
        }

        self.push(&format!("fn {}(", name));
        self.emit_fn_params(&f.function.params)?;
        self.push(")");

        if let Some(ret) = &f.function.return_type {
            self.push(" -> ");
            self.emit_ts_type(&ret.type_ann)?;
        }

        if let Some(body) = &f.function.body {
            self.push(" ");
            self.emit_block_stmt(body)?;
        } else {
            self.push(";")?;
        }

        Ok(())
    }

    /// Emit function parameters.
    fn emit_fn_params(&mut self, params: &[Param]) -> crate::Result<()> {
        for (i, param) in params.iter().enumerate() {
            if i > 0 { self.push(", "); }
            self.emit_fn_param(param)?;
        }
        Ok(())
    }

    /// Emit function parameter.
    fn emit_fn_param(&mut self, param: &Param) -> crate::Result<()> {
        let name = param.pat.as_ident()
            .map(|i| self.mangle(&i.id.sym.to_string()))
            .unwrap_or_else(|| "_".to_string());

        let mode = self.analysis.ownership.get(&name).copied().unwrap_or(BorrowMode::Unknown);
        let prefix = match mode {
            BorrowMode::Mut => "&mut ",
            BorrowMode::Shared => "&",
            BorrowMode::Owned | BorrowMode::Unknown => "",
        };

        if let Some(type_ann) = param.pat.as_ident().and_then(|i| i.type_ann.as_ref()) {
            self.push(&format!("{prefix}{name}: "));
            self.emit_ts_type(&type_ann.type_ann)?;
        } else {
            self.push(&format!("{prefix}{name}"));
        }

        Ok(())
    }

    /// Emit a block statement.
    fn emit_block_stmt(&mut self, block: &BlockStmt) -> crate::Result<()> {
        self.push_line("{")?;
        self.indent += 1;

        for stmt in &block.stmts {
            let mut emitter = StmtEmitter::new(self.analysis);
            emitter.emit_stmt(stmt);
            self.push(&emitter.into_output());
        }

        self.indent -= 1;
        self.push("}")?;
        Ok(())
    }

    /// Emit a variable declaration.
    fn emit_var_decl(&mut self, v: &VarDecl) -> crate::Result<()> {
        let keyword = match v.kind {
            VarDeclKind::Const => "let",
            VarDeclKind::Let | VarDeclKind::Var => "let mut",
        };

        for (i, decl) in v.decls.iter().enumerate() {
            if i > 0 {
                self.push_line(";");
                self.push_indent();
            }
            self.push(&format!("{} ", keyword));
            self.emit_pat(&decl.name)?;
            if let Some(init) = &decl.init {
                self.push(" = ");
                let mut expr_emitter = ExprEmitter::new(self.analysis);
                self.push(&expr_emitter.emit_expr(init));
            }
        }
        Ok(())
    }

    /// Emit a pattern.
    fn emit_pat(&mut self, pat: &Pat) -> crate::Result<()> {
        match pat {
            Pat::Ident(i) => {
                let name = self.mangle(&i.id.sym.to_string());
                self.push(&name);
                if let Some(type_ann) = &i.type_ann {
                    self.push(": ");
                    self.emit_ts_type(&type_ann.type_ann)?;
                }
            }
            Pat::Array(a) => {
                self.push("[");
                for (i, elem) in a.elems.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    if let Some(p) = elem {
                        self.emit_pat(p)?;
                    }
                }
                self.push("]");
            }
            Pat::Object(o) => {
                self.push("{");
                for (i, prop) in o.props.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    match prop {
                        ObjectPatProp::Assign(a) => {
                            self.push(&a.key.sym.to_string());
                        }
                        ObjectPatProp::KeyValue(kv) => {
                            let mut expr_emitter = ExprEmitter::new(self.analysis);
                            self.push(&expr_emitter.emit_expr(&Expr::Ident(kv.key.clone().into()))?);
                            self.push(": ");
                            self.emit_pat(&kv.value)?;
                        }
                        ObjectPatProp::Rest(r) => {
                            self.push("..");
                            self.emit_pat(&r.arg)?;
                        }
                    }
                }
                self.push("}");
            }
            Pat::Rest(r) => {
                self.push("..");
                self.emit_pat(&r.arg)?;
            }
            _ => self.push("_"),
        }
        Ok(())
    }

    /// Emit a type alias.
    fn emit_type_alias(&mut self, t: &TsTypeAliasDecl) -> crate::Result<()> {
        self.push(&format!("pub type {} = ", self.mangle(&t.id.sym.to_string())));
        self.emit_ts_type(&t.type_ann)?;
        self.push(";")?;
        Ok(())
    }

    /// Emit a TypeScript enum.
    fn emit_enum(&mut self, e: &TsEnumDecl) -> crate::Result<()> {
        self.push(&format!("pub enum {}", self.mangle(&e.id.sym.to_string())));
        self.push_line(" {")?;

        for member in &e.members {
            let tag = match &member.id {
                TsEnumMemberId::Str(s) => s.value.to_string(),
                TsEnumMemberId::Computed(_) => continue,
            };
            self.push_indent();
            self.push(&format!("{},", self.pascal_case(&tag)));
            self.push_line("");
        }

        self.push("}")?;
        Ok(())
    }

    /// Emit an interface.
    fn emit_interface(&mut self, i: &TsInterfaceDecl) -> crate::Result<()> {
        self.push(&format!("pub struct {} ", self.mangle(&i.id.sym.to_string())))?;
        self.emit_block_stmt(&BlockStmt {
            span: Default::default(),
            stmts: Vec::new(),
        })?;
        Ok(())
    }

    /// Emit a module declaration.
    fn emit_module_decl(&mut self, decl: &ModuleDecl) -> crate::Result<()> {
        match decl {
            ModuleDecl::Import(i) => self.emit_import(i),
            ModuleDecl::Export(e) => self.emit_export(e),
            _ => Ok(()),
        }
    }

    /// Emit an import.
    fn emit_import(&mut self, import: &ImportDecl) -> crate::Result<()> {
        let module_name = import.src.value.to_string();

        if module_name.starts_with("native:") {
            let path = module_name.strip_prefix("native:").unwrap_or(&module_name);
            let import = Import {
                path: format!("crate::native::{}", path.replace(".", "::")),
                names: import.specifiers.iter().filter_map(|s| {
                    match s {
                        ImportSpecifier::Named(n) => Some(ImportedName {
                            original: n.local.sym.to_string(),
                            rust_name: self.snake_case(&n.local.sym.to_string()),
                        }),
                        ImportSpecifier::Default(n) => Some(ImportedName {
                            original: "default".to_string(),
                            rust_name: n.sym.to_string(),
                        }),
                        ImportSpecifier::Namespace(n) => Some(ImportedName {
                            original: "*".to_string(),
                            rust_name: n.sym.to_string(),
                        }),
                    }
                }).collect(),
                is_native: true,
            };
            self.imports.push(import);
        } else if module_name.starts_with("./") || module_name.starts_with("../") {
            let import = Import {
                path: format!("crate::generated::{}",
                    module_name.replace("./", "").replace(".r.ts", "").replace(".r.tsx", "")),
                names: import.specifiers.iter().filter_map(|s| {
                    match s {
                        ImportSpecifier::Named(n) => Some(ImportedName {
                            original: n.local.sym.to_string(),
                            rust_name: self.snake_case(&n.local.sym.to_string()),
                        }),
                        _ => None,
                    }
                }).collect(),
                is_native: false,
            };
            self.imports.push(import);
        }

        Ok(())
    }

    /// Emit an export.
    fn emit_export(&mut self, export: &ExportDecl) -> crate::Result<()> {
        match export {
            ExportDecl::Decl(d) => self.emit_decl(d),
            ExportDecl::Named(n) => {
                for spec in &n.specifiers {
                    if let ExportSpecifier::Named(ns) = spec {
                        self.push(&format!("pub use {};", self.mangle(&ns.orig.sym.to_string())))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Emit a TypeScript type.
    fn emit_ts_type(&mut self, ts_type: &TsType) -> crate::Result<()> {
        match ts_type {
            TsType::TsKeywordType(k) => {
                let ty = match k.kind {
                    TsKeywordTypeKind::TsNumberKeyword => "f64",
                    TsKeywordTypeKind::TsStringKeyword => "String",
                    TsKeywordTypeKind::TsBooleanKeyword => "bool",
                    TsKeywordTypeKind::TsNullKeyword => "()",
                    TsKeywordTypeKind::TsUndefinedKeyword => "()",
                    TsKeywordTypeKind::TsVoidKeyword => "()",
                    _ => "()",
                };
                self.push(ty);
            }
            TsType::TsArrayType(a) => {
                self.emit_ts_type(&a.elem_type)?;
                self.push("Vec<");
                self.emit_ts_type(&a.elem_type)?;
                self.push(">");
            }
            TsType::TsTypeRef(t) => {
                let name = t.type_name.as_str();
                match name {
                    "Array" | "Vec" => {
                        self.push("Vec<");
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                self.emit_ts_type(&params.params[0])?;
                            }
                        }
                        self.push(">");
                    }
                    "Option" => {
                        self.push("Option<");
                        if let Some(params) = &t.type_params {
                            if !params.params.is_empty() {
                                self.emit_ts_type(&params.params[0])?;
                            }
                        }
                        self.push(">");
                    }
                    "Result" => {
                        self.push("Result<");
                        if let Some(params) = &t.type_params {
                            if params.params.len() >= 2 {
                                self.emit_ts_type(&params.params[0])?;
                                self.push(", ");
                                self.emit_ts_type(&params.params[1])?;
                            }
                        }
                        self.push(">");
                    }
                    "string" => self.push("String"),
                    "number" => self.push("f64"),
                    "boolean" => self.push("bool"),
                    "void" => self.push("()"),
                    _ => self.push(&self.mangle(name)),
                }
            }
            TsType::TsTupleType(t) => {
                self.push("(");
                for (i, elem) in t.elem_types.iter().enumerate() {
                    if i > 0 { self.push(", "); }
                    self.emit_ts_type(&elem.ty)?;
                }
                self.push(")");
            }
            TsType::TsParenthesizedType(p) => {
                self.emit_ts_type(&p.type_ann)?;
            }
            _ => self.push("()"),
        }
        Ok(())
    }

    // Utility methods

    fn push(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn mangle(&self, name: &str) -> String {
        if matches!(
            name,
            "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn"
            | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl"
            | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub"
            | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
            | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while"
        ) {
            format!("{}_", name)
        } else {
            name.to_string()
        }
    }

    fn snake_case(&self, s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        }
        result
    }

    fn pascal_case(&self, s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;
        for c in s.chars() {
            if c == '_' || c == '-' || c == ' ' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_uppercase().next().unwrap_or(c));
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
        result
    }
}
