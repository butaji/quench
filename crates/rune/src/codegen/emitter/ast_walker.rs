//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.
//!
//! This is a simplified walker that handles the most common TypeScript patterns
//! for Rune's zero-overhead subset.

use swc_ecma_ast::{
    Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem, Stmt,
};
use std::collections::HashMap;

/// Type information for code generation.
#[derive(Debug, Clone)]
pub enum RustType {
    I32,
    F64,
    Bool,
    String,
    Str,
    Vec(Box<RustType>),
    Option(Box<RustType>),
    Result(Box<RustType>),
    HashMap(Box<RustType>, Box<RustType>),
    Unit,
    Unknown,
    Custom(String),
}

impl RustType {
    /// Convert to Rust type string.
    pub fn to_string(&self) -> String {
        match self {
            RustType::I32 => "i32".to_string(),
            RustType::F64 => "f64".to_string(),
            RustType::Bool => "bool".to_string(),
            RustType::String => "String".to_string(),
            RustType::Str => "&str".to_string(),
            RustType::Vec(t) => format!("Vec<{}>", t.to_string()),
            RustType::Option(t) => format!("Option<{}>", t.to_string()),
            RustType::Result(t) => format!("Result<{}, String>", t.to_string()),
            RustType::HashMap(k, v) => format!("std::collections::HashMap<{}, {}>", k.to_string(), v.to_string()),
            RustType::Unit => "()".to_string(),
            RustType::Unknown => "()".to_string(),
            RustType::Custom(name) => name.clone(),
        }
    }
}

/// Raw field for deferred type resolution.
type RawField = (String, swc_ecma_ast::TsType);

/// Type alias for storing struct field info.
type StructFields = Vec<(String, RustType)>;

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    /// Output buffer for accumulating code
    output: String,
    /// Current indentation level
    indent: usize,
    /// Collected type definitions (name -> fields)
    type_fields: HashMap<String, Vec<RawField>>,
    /// Collected enum definitions
    enums: HashMap<String, EnumDefinition>,
    /// Anonymous struct counter for generating unique names
    anonymous_struct_counter: usize,
    /// Pending anonymous structs to emit
    pending_anonymous_structs: Vec<(String, StructFields)>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: StructFields,
}

#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

impl AstWalker {
    /// Create a new AST walker.
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            type_fields: HashMap::new(),
            enums: HashMap::new(),
            anonymous_struct_counter: 0,
            pending_anonymous_structs: Vec::new(),
        }
    }

    /// Walk a module and emit Rust code.
    pub fn walk_module(&mut self, module: &Module) {
        // First pass: collect all type definitions and enums
        for item in &module.body {
            self.collect_item(item);
        }

        // Second pass: emit type definitions (named structs and enums)
        // Note: anonymous structs are emitted after functions since they may be
        // referenced in function parameters
        self.emit_named_types();

        // Third pass: emit functions (this may add to pending_anonymous_structs)
        for item in &module.body {
            self.emit_item(item);
        }

        // Fourth pass: emit any remaining anonymous structs referenced in functions
        self.emit_anonymous_structs();
    }

    /// Emit named type definitions.
    fn emit_named_types(&mut self) {
        // Clone data first to avoid borrow conflict
        let type_names: Vec<String> = self.type_fields.keys().cloned().collect();
        let type_data: Vec<(String, Vec<RawField>)> = type_names.iter()
            .filter_map(|n| self.type_fields.get(n).map(|f| (n.clone(), f.clone())))
            .collect();
        
        for (name, raw_fields) in type_data {
            let fields: StructFields = raw_fields.iter()
                .map(|(n, t)| (n.clone(), self.resolve_type(t)))
                .collect();
            self.emit_struct(&name, &fields);
        }

        // Emit enum types - clone first
        let enum_data: Vec<(String, EnumDefinition)> = self.enums.iter()
            .filter_map(|(n, e)| Some((n.clone(), e.clone())))
            .collect();
        for (_, ed) in enum_data {
            self.emit_enum(&ed);
        }
    }

    /// Emit anonymous struct definitions.
    fn emit_anonymous_structs(&mut self) {
        let anon_structs = std::mem::take(&mut self.pending_anonymous_structs);
        for (name, fields) in anon_structs {
            self.emit_struct(&name, &fields);
        }
    }

    /// Collect declarations from a module item.
    #[allow(clippy::single_match)]
    fn collect_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::TsInterface(d))) => {
                self.collect_interface(&d.id.sym.to_string(), &d.body);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsEnum(d))) => {
                self.collect_enum(&d.id.sym.to_string(), d);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsTypeAlias(d))) => {
                self.collect_type_alias(&d.id.sym.to_string(), &d.type_ann);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsModule(d))) => {
                self.collect_ts_module(d);
            }
            _ => {}
        }
    }

    /// Collect interface/type definition.
    fn collect_interface(&mut self, name: &str, body: &swc_ecma_ast::TsInterfaceBody) {
        let mut fields: Vec<(String, swc_ecma_ast::TsType)> = Vec::new();
        for member in &body.body {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = match prop.key.as_ref() {
                    swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) => format!("{:?}", s.value),
                    _ => "_unknown".to_string(),
                };
                
                if let Some(type_ann) = &prop.type_ann {
                    // Dereference the Box<TsType> to get TsType
                    fields.push((field_name, (*type_ann.type_ann).clone()));
                }
            }
        }
        self.type_fields.insert(name.to_string(), fields);
    }

    /// Collect enum definition.
    fn collect_enum(&mut self, name: &str, decl: &swc_ecma_ast::TsEnumDecl) {
        let mut variants = Vec::new();
        for member in &decl.members {
            let var_name = format!("{:?}", member.id);
            variants.push(EnumVariant {
                name: var_name,
                fields: Vec::new(),
            });
        }
        self.enums.insert(name.to_string(), EnumDefinition {
            name: name.to_string(),
            variants,
        });
    }

    /// Collect type alias.
    fn collect_type_alias(&mut self, name: &str, type_ann: &swc_ecma_ast::TsType) {
        // Store type alias - we'll resolve it when emitting
        self.type_fields.insert(name.to_string(), vec![("_type".to_string(), type_ann.clone())]);
    }

    /// Collect TypeScript module/namespace.
    fn collect_ts_module(&mut self, decl: &Box<swc_ecma_ast::TsModuleDecl>) {
        if let Some(body) = decl.body.as_ref() {
            if let swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block) = body {
                for item in &block.body {
                    self.collect_item(item);
                }
            }
        }
    }

    /// Resolve a raw TypeScript type to RustType.
    fn resolve_type(&mut self, ts_type: &swc_ecma_ast::TsType) -> RustType {
        match ts_type {
            swc_ecma_ast::TsType::TsKeywordType(k) => {
                match k.kind {
                    swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => RustType::F64,
                    swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => RustType::String,
                    swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => RustType::Bool,
                    swc_ecma_ast::TsKeywordTypeKind::TsVoidKeyword => RustType::Unit,
                    swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword => RustType::Unknown,
                    swc_ecma_ast::TsKeywordTypeKind::TsUndefinedKeyword => RustType::Unit,
                    _ => RustType::Unknown,
                }
            }
            swc_ecma_ast::TsType::TsArrayType(arr) => {
                RustType::Vec(Box::new(self.resolve_type(&arr.elem_type)))
            }
            swc_ecma_ast::TsType::TsTypeRef(type_ref) => {
                let name = match &type_ref.type_name {
                    swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
                };
                
                if name == "null" {
                    return RustType::Unknown;
                }
                
                if let Some(params) = &type_ref.type_params {
                    if !params.params.is_empty() {
                        let inner = self.resolve_type(&params.params[0]);
                        if name == "Array" {
                            return RustType::Vec(Box::new(inner));
                        }
                    }
                }
                
                RustType::Custom(name)
            }
            swc_ecma_ast::TsType::TsUnionOrIntersectionType(union) => {
                match union {
                    swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u) => {
                        if u.types.len() == 2 {
                            let has_null = u.types.iter().any(|t| {
                                if let swc_ecma_ast::TsType::TsKeywordType(k) = t.as_ref() {
                                    k.kind == swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword
                                } else {
                                    false
                                }
                            });
                            if has_null {
                                let non_null = u.types.iter().find(|t| {
                                    if let swc_ecma_ast::TsType::TsKeywordType(k) = t.as_ref() {
                                        k.kind != swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword
                                    } else {
                                        true
                                    }
                                });
                                if let Some(t) = non_null {
                                    return RustType::Option(Box::new(self.resolve_type(t)));
                                }
                            }
                        }
                    }
                    swc_ecma_ast::TsUnionOrIntersectionType::TsIntersectionType(_) => {}
                }
                RustType::Unknown
            }
            swc_ecma_ast::TsType::TsParenthesizedType(paren) => {
                self.resolve_type(&paren.type_ann)
            }
            swc_ecma_ast::TsType::TsTupleType(_tuple) => {
                RustType::Unknown
            }
            swc_ecma_ast::TsType::TsTypeLit(lit) => {
                // Generate anonymous struct
                let mut fields = Vec::new();
                let mut field_counter = 0;
                for member in &lit.members {
                    if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                        // Get field name from the key
                        let field_name = match prop.key.as_ref() {
                            swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                            _ => {
                                field_counter += 1;
                                format!("_field{}", field_counter)
                            }
                        };
                        let field_type = if let Some(ann) = &prop.type_ann {
                            self.resolve_type(&ann.type_ann)
                        } else {
                            RustType::Unknown
                        };
                        fields.push((field_name, field_type));
                    }
                }
                
                self.anonymous_struct_counter += 1;
                let struct_name = format!("__AnonymousStruct{}", self.anonymous_struct_counter);
                self.pending_anonymous_structs.push((struct_name.clone(), fields));
                RustType::Custom(struct_name)
            }
            _ => RustType::Unknown,
        }
    }



    /// Emit a struct definition.
    fn emit_struct(&mut self, name: &str, fields: &StructFields) {
        let struct_name = if name.starts_with("__") {
            name.to_string()
        } else {
            to_pascal_case(name)
        };
        self.push_line(&format!("#[derive(Debug, Clone)]"));
        self.push_line(&format!("pub struct {} {{", struct_name));
        self.indent += 1;
        for (field_name, field_type) in fields {
            let rust_field = to_snake_case(field_name);
            self.push_line(&format!("pub {}: {},", rust_field, field_type.to_string()));
        }
        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit an enum definition.
    fn emit_enum(&mut self, ed: &EnumDefinition) {
        let pascal_name = to_pascal_case(&ed.name);
        self.push_line(&format!("#[derive(Debug, Clone, Copy, PartialEq)]"));
        self.push_line(&format!("pub enum {} {{", pascal_name));
        self.indent += 1;
        for variant in &ed.variants {
            if variant.fields.is_empty() {
                self.push_line(&format!("{},", to_pascal_case(&variant.name)));
            } else {
                let fields: Vec<String> = variant.fields.iter()
                    .map(|(n, t)| format!("{}: {}", to_snake_case(n), t.to_string()))
                    .collect();
                self.push_line(&format!("{} {{ {} }},", to_pascal_case(&variant.name), fields.join(", ")));
            }
        }
        self.indent -= 1;
        self.push_line("}");
        self.push_line("");
    }

    /// Emit code from a module item.
    #[allow(clippy::single_match)]
    fn emit_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::Fn(fn_decl))) => {
                self.emit_function(fn_decl);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) => {
                if let Decl::Fn(fn_decl) = decl {
                    self.emit_function(fn_decl);
                }
            }
            _ => {}
        }
    }

    /// Emit a function.
    fn emit_function(&mut self, fn_decl: &FnDecl) {
        let fn_name = fn_decl.ident.sym.to_string();
        let rust_name = to_snake_case(&fn_name);

        // Get parameter types
        let params: Vec<(String, RustType)> = fn_decl.function.params.iter()
            .filter_map(|p| {
                match &p.pat {
                    swc_ecma_ast::Pat::Ident(ident) => {
                        let name = ident.id.sym.to_string();
                        let ty = if let Some(ann) = &ident.type_ann {
                            self.resolve_type(&ann.type_ann)
                        } else {
                            RustType::Unknown
                        };
                        Some((name, ty))
                    }
                    _ => None,
                }
            })
            .collect();

        // Get return type
        let return_type = if let Some(ret) = &fn_decl.function.return_type {
            self.resolve_type(&ret.type_ann)
        } else {
            RustType::Unit
        };

        let is_async = fn_decl.function.is_async;

        // Emit function signature
        let async_prefix = if is_async { "async " } else { "" };
        let params_str: Vec<String> = params.iter()
            .map(|(n, t)| format!("{}: {}", to_snake_case(n), t.to_string()))
            .collect();

        self.push_indent();
        self.push_str(&format!("pub {}fn {}({}) -> {} {{\n",
            async_prefix,
            rust_name,
            params_str.join(", "),
            return_type.to_string()
        ));
        self.indent += 1;

        // Emit function body (placeholder for now)
        self.push_indent();
        self.push_line("// TODO: implement function body");
        self.push_indent();
        self.push_line("unimplemented!()");
        self.indent -= 1;

        self.push_line("}");
        self.push_line("");
    }

    /// Push a line to output.
    fn push_line(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Push a string to output without newline.
    fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Push indentation.
    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    /// Get the generated output.
    pub fn into_output(self) -> String {
        self.output
    }
}

impl Default for AstWalker {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert name to snake_case.
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Convert name to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}
