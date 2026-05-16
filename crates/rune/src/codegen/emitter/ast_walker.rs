//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.

use super::{CodeEmitter, TypeResolver, RustType, to_snake_case};
use super::types::to_pascal_case;
use swc_ecma_ast::{
    Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem,
    Stmt, TsType,
};
use std::collections::HashMap;
use super::types::{EnumDefinition, EnumVariant};

/// Raw field for deferred type resolution.
type RawField = (String, TsType);

/// Information about a struct type.
#[derive(Debug, Clone)]
struct StructInfo {
    /// Rust struct name
    rust_name: String,
    /// Fields with types
    fields: Vec<(String, RustType)>,
}

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    /// Type fields to emit (TS name → fields)
    type_fields: HashMap<String, Vec<RawField>>,
    /// Enums to emit
    enums: HashMap<String, EnumDefinition>,
    /// Struct type info (TS name → struct info)
    structs: HashMap<String, StructInfo>,
    /// Code emitter
    emitter: CodeEmitter,
    /// Type resolver
    resolver: TypeResolver,
    /// Module name (for imports)
    module_name: String,
    /// Known imports
    imports: HashMap<String, Vec<String>>,
}

impl AstWalker {
    /// Create a new AST walker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_fields: HashMap::new(),
            enums: HashMap::new(),
            structs: HashMap::new(),
            emitter: CodeEmitter::new(),
            resolver: TypeResolver::new(),
            module_name: String::new(),
            imports: HashMap::new(),
        }
    }

    /// Walk a module and emit Rust code.
    pub fn walk_module(&mut self, module: &Module) {
        // First pass: collect imports
        self.collect_imports(module);

        // Second pass: collect all type definitions and enums
        for item in &module.body {
            self.collect_item(item);
        }
        self.emit_named_types();

        // Third pass: emit functions
        for item in &module.body {
            self.emit_item(item);
        }
        self.emit_anonymous_structs();
    }

    fn collect_imports(&mut self, module: &Module) {
        for item in &module.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                // import.src is a Box<Str> containing the module path (ESM import)
                let path_str = format!("{:?}", import.src.value);
                let names: Vec<String> = import
                    .specifiers
                    .iter()
                    .map(|spec| match spec {
                        swc_ecma_ast::ImportSpecifier::Named(named) => {
                            to_snake_case(named.local.as_ref())
                        }
                        swc_ecma_ast::ImportSpecifier::Default(_) => {
                            "default".to_string()
                        }
                        swc_ecma_ast::ImportSpecifier::Namespace(ns) => {
                            format!("*{}", to_snake_case(ns.local.as_ref()))
                        }
                    })
                    .collect();
                self.imports.insert(path_str, names);
            }
        }
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

            // Store struct info for object literal context
            let rust_name = to_pascal_case(&name);
            self.structs.insert(name.clone(), StructInfo {
                rust_name: rust_name.clone(),
                fields: fields.clone(),
            });

            self.emitter.emit_struct(&rust_name, &fields);
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
        let mut fields: Vec<(String, TsType)> = Vec::new();
        for member in &body.body {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    ident.sym.to_string()
                } else if let swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) =
                    prop.key.as_ref()
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

    fn collect_type_alias(&mut self, name: &str, type_ann: &TsType) {
        // Check if this is a struct type (object literal)
        if let TsType::TsTypeLit(lit) = type_ann {
            let mut fields: Vec<(String, TsType)> = Vec::new();
            for member in &lit.members {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                    let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                        ident.sym.to_string()
                    } else if let swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) =
                        prop.key.as_ref()
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
            // Store as a struct type, not just a raw field
            let rust_name = to_pascal_case(name);
            let resolved_fields: Vec<(String, RustType)> = fields
                .iter()
                .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
                .collect();
            self.structs.insert(
                name.to_string(),
                StructInfo {
                    rust_name,
                    fields: resolved_fields,
                },
            );
            // Also store in type_fields for backward compatibility
            self.type_fields.insert(name.to_string(), fields);
            return;
        }

        // Check if this is a union type (tagged union)
        if let TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann
        {
            if u.types.iter().all(|t| self.is_tagged_variant(t)) {
                self.collect_tagged_union(name, &u.types);
                return;
            }
        }

        // For other type aliases, just store the type reference
        self.type_fields
            .insert(name.to_string(), vec![("_type".to_string(), type_ann.clone())]);
    }

    /// Check if a type is a tagged variant.
    fn is_tagged_variant(&self, ts_type: &TsType) -> bool {
        if let TsType::TsTypeLit(lit) = ts_type {
            lit.members.iter().any(|m| {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = m {
                    if let Some(type_ann) = &prop.type_ann {
                        if let TsType::TsKeywordType(k) = type_ann.type_ann.as_ref() {
                            return k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword;
                        }
                    }
                }
                false
            })
        } else {
            false
        }
    }

    /// Collect a tagged union (enum).
    fn collect_tagged_union(&mut self, name: &str, types: &[Box<TsType>]) {
        let mut variants: Vec<EnumVariant> = Vec::new();

        for ts_type in types {
            if let TsType::TsTypeLit(lit) = ts_type.as_ref() {
                let mut tag = String::new();
                let mut fields: Vec<(String, RustType)> = Vec::new();

                for member in &lit.members {
                    if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                        let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                            ident.sym.to_string()
                        } else {
                            continue;
                        };

                        if let Some(type_ann) = &prop.type_ann {
                            let ty = self.resolver.resolve(&type_ann.type_ann);
                            if field_name == "tag" {
                                // Extract the string literal value for the tag
                                if let TsType::TsKeywordType(k) = type_ann.type_ann.as_ref() {
                                    if k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword {
                                        tag = format!("{}{}",
                                            name.chars().next().unwrap().to_uppercase(),
                                            &name[1..]
                                        );
                                    }
                                }
                            } else {
                                fields.push((field_name, ty));
                            }
                        }
                    }
                }

                if !tag.is_empty() {
                    variants.push(EnumVariant { name: tag, fields });
                }
            }
        }

        if !variants.is_empty() {
            self.enums.insert(
                name.to_string(),
                EnumDefinition {
                    name: name.to_string(),
                    variants,
                },
            );
        }
    }

    fn collect_ts_module(&mut self, decl: &swc_ecma_ast::TsModuleDecl) {
        if let Some(swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block)) = decl.body.as_ref()
        {
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
        let body =
            fn_decl
                .function
                .body
                .as_ref()
                .map(|block| Stmt::Block(block.clone()));

        // Set the expected return type for type inference
        self.emitter.set_expected_return(Some(return_type.to_string()));

        self.emitter.emit_function_with_body(
            &rust_name,
            &params,
            &return_type,
            is_async,
            body,
        );

        self.emitter.set_expected_return(None);
    }

    /// Escape a Rust keyword for use as an identifier.
    #[must_use]
    pub fn escape_keyword(name: &str) -> String {
        match name {
            "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn"
            | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl"
            | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref"
            | "return" | "self" | "Self" | "static" | "struct" | "super" | "trait" | "true"
            | "type" | "unsafe" | "use" | "where" | "while" | "abstract" | "become"
            | "box" | "do" | "final" | "macro" | "override" | "priv" | "try"
            | "typeof" | "unsized" | "virtual" | "yield" => format!("r#{name}"),
            _ => name.to_string(),
        }
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
