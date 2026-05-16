//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.

use super::{CodeEmitter, TypeResolver, RustType, to_snake_case};
use swc_ecma_ast::{Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem, Stmt};
use std::collections::HashMap;
use super::types::{EnumDefinition, EnumVariant};

/// Raw field for deferred type resolution.
type RawField = (String, swc_ecma_ast::TsType);

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    /// Type fields to emit
    type_fields: HashMap<String, Vec<RawField>>,
    /// Enums to emit
    enums: HashMap<String, EnumDefinition>,
    /// Code emitter
    emitter: CodeEmitter,
    /// Type resolver
    resolver: TypeResolver,
}

impl AstWalker {
    /// Create a new AST walker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_fields: HashMap::new(),
            enums: HashMap::new(),
            emitter: CodeEmitter::new(),
            resolver: TypeResolver::new(),
        }
    }

    /// Walk a module and emit Rust code.
    pub fn walk_module(&mut self, module: &Module) {
        // First pass: collect all type definitions and enums
        for item in &module.body {
            self.collect_item(item);
        }
        self.emit_named_types();

        // Second pass: emit functions
        for item in &module.body {
            self.emit_item(item);
        }
        self.emit_anonymous_structs();
    }

    fn emit_named_types(&mut self) {
        // Collect type data to avoid borrow conflicts
        let type_data: Vec<(String, Vec<RawField>)> = self
            .type_fields
            .iter()
            .map(|(n, f)| (n.clone(), f.clone()))
            .collect();

        for (name, raw_fields) in type_data {
            let fields: Vec<(String, RustType)> = raw_fields
                .iter()
                .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
                .collect();
            self.emitter.emit_struct(&name, &fields);
        }

        // Emit enum types
        let enum_data: Vec<EnumDefinition> = self.enums.values().cloned().collect();
        for ed in enum_data {
            self.emitter.emit_enum(&ed);
        }
    }

    fn emit_anonymous_structs(&mut self) {
        let anon_structs = self.resolver.take_pending_structs();
        for (name, fields) in anon_structs {
            self.emitter.emit_struct(&name, &fields);
        }
    }

    #[allow(clippy::single_match)]
    fn collect_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::TsInterface(d))) => {
                self.collect_interface(d.id.sym.as_ref(), &d.body);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsEnum(d))) => {
                self.collect_enum(d.id.sym.as_ref(), d);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsTypeAlias(d))) => {
                self.collect_type_alias(d.id.sym.as_ref(), &d.type_ann);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsModule(d))) => {
                self.collect_ts_module(d);
            }
            _ => {}
        }
    }

    fn collect_interface(&mut self, name: &str, body: &swc_ecma_ast::TsInterfaceBody) {
        let mut fields: Vec<(String, swc_ecma_ast::TsType)> = Vec::new();
        for member in &body.body {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    ident.sym.to_string()
                } else if let swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) = prop.key.as_ref()
                {
                    format!("{:?}", s.value)
                } else {
                    "_unknown".to_string()
                };

                if let Some(type_ann) = &prop.type_ann {
                    fields.push((field_name, (*type_ann.type_ann).clone()));
                }
            }
        }
        self.type_fields.insert(name.to_string(), fields);
    }

    fn collect_enum(&mut self, name: &str, decl: &swc_ecma_ast::TsEnumDecl) {
        let variants: Vec<EnumVariant> = decl
            .members
            .iter()
            .map(|member| EnumVariant {
                name: format!("{:?}", member.id),
                fields: Vec::new(),
            })
            .collect();

        self.enums.insert(
            name.to_string(),
            EnumDefinition {
                name: name.to_string(),
                variants,
            },
        );
    }

    fn collect_type_alias(&mut self, name: &str, type_ann: &swc_ecma_ast::TsType) {
        self.type_fields
            .insert(name.to_string(), vec![("_type".to_string(), type_ann.clone())]);
    }

    fn collect_ts_module(&mut self, decl: &swc_ecma_ast::TsModuleDecl) {
        if let Some(swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block)) = decl.body.as_ref() {
            for item in &block.body {
                self.collect_item(item);
            }
        }
    }

    #[allow(clippy::collapsible_match)]
    fn emit_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::Fn(fn_decl))) => {
                self.emit_function(fn_decl);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: Decl::Fn(fn_decl),
                ..
            })) => {
                self.emit_function(fn_decl);
            }
            _ => {}
        }
    }

    fn emit_function(&mut self, fn_decl: &FnDecl) {
        let rust_name = to_snake_case(fn_decl.ident.sym.as_ref());

        let params: Vec<(String, RustType)> = fn_decl
            .function
            .params
            .iter()
            .filter_map(|p| {
                if let swc_ecma_ast::Pat::Ident(ident) = &p.pat {
                    let ty = ident
                        .type_ann
                        .as_ref()
                        .map_or(RustType::Unknown, |ann| self.resolver.resolve(&ann.type_ann));
                    Some((ident.id.sym.to_string(), ty))
                } else {
                    None
                }
            })
            .collect();

        let return_type = fn_decl
            .function
            .return_type
            .as_ref()
            .map_or(RustType::Unit, |ann| self.resolver.resolve(&ann.type_ann));

        let is_async = fn_decl.function.is_async;

        // Get the function body
        let body = fn_decl.function.body.as_ref().map(|block| Stmt::Block(block.clone()));

        self.emitter.emit_function_with_body(
            &rust_name,
            &params,
            &return_type,
            is_async,
            body,
        );
    }

    /// Get the generated output.
    #[must_use]
    pub fn output(&self) -> &str {
        self.emitter.output()
    }

    /// Consume walker and return output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.emitter.into_output()
    }
}

impl Default for AstWalker {
    fn default() -> Self {
        Self::new()
    }
}
