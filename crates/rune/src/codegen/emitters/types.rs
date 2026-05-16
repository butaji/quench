//! # Type Emitter
//!
//! Emits Rust type declarations from TypeScript types.

use swc_ecma_ast::*;

/// Emits Rust type code.
pub struct TypeEmitter {
    /// Output buffer
    output: String,
}

impl TypeEmitter {
    /// Create a new type emitter.
    pub fn new() -> Self {
        Self { output: String::new() }
    }

    /// Emit a TypeScript type.
    pub fn emit(&mut self, ts_type: &TsType) -> String {
        self.emit_ts_type(ts_type);
        std::mem::take(&mut self.output)
    }

    /// Emit a TypeScript type to output.
    fn emit_ts_type(&mut self, ts_type: &TsType) {
        match ts_type {
            TsType::TsKeywordType(k) => self.emit_keyword(k),
            TsType::TsArrayType(a) => self.emit_array(a),
            TsType::TsTypeRef(t) => self.emit_type_ref(t),
            TsType::TsTupleType(t) => self.emit_tuple(t),
            TsType::TsParenthesizedType(p) => self.emit_ts_type(&p.type_ann),
            _ => self.push("()"),
        }
    }

    /// Emit a keyword type.
    fn emit_keyword(&mut self, k: &TsKeywordType) {
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

    /// Emit an array type.
    fn emit_array(&mut self, a: &TsArrayType) {
        self.emit_ts_type(&a.elem_type);
        self.push("Vec<");
        self.emit_ts_type(&a.elem_type);
        self.push(">");
    }

    /// Emit a type reference.
    fn emit_type_ref(&mut self, t: &TsTypeRef) {
        let name = t.type_name.as_str();
        match name {
            "Array" | "Vec" => {
                self.push("Vec<");
                if let Some(params) = &t.type_params {
                    if !params.params.is_empty() {
                        self.emit_ts_type(&params.params[0]);
                    }
                }
                self.push(">");
            }
            "Option" => {
                self.push("Option<");
                if let Some(params) = &t.type_params {
                    if !params.params.is_empty() {
                        self.emit_ts_type(&params.params[0]);
                    }
                }
                self.push(">");
            }
            "Result" => {
                self.push("Result<");
                if let Some(params) = &t.type_params {
                    if params.params.len() >= 2 {
                        self.emit_ts_type(&params.params[0]);
                        self.push(", ");
                        self.emit_ts_type(&params.params[1]);
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

    /// Emit a tuple type.
    fn emit_tuple(&mut self, t: &TsTupleType) {
        self.push("(");
        for (i, elem) in t.elem_types.iter().enumerate() {
            if i > 0 { self.push(", "); }
            self.emit_ts_type(&elem.ty);
        }
        self.push(")");
    }

    /// Push string to output.
    fn push(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Mangle a name to avoid keyword conflicts.
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
}

impl Default for TypeEmitter {
    fn default() -> Self {
        Self::new()
    }
}
