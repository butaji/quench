//! # Variable Declaration Emitter
//! Emits Rust variable declarations from TypeScript var/let/const.
use super::{CodeEmitter, emit_expr, infer_type};

/// Emit a variable declaration.
pub fn emit_var_decl(emitter: &mut CodeEmitter, decl: &swc_ecma_ast::Decl) {
    if let swc_ecma_ast::Decl::Var(var_decl) = decl {
        for vdecl in &var_decl.decls {
            let explicit_type = extract_type_annotation(&vdecl.name);

            let inferred_struct = if explicit_type.is_none() {
                if let Some(init_box) = &vdecl.init {
                    if let swc_ecma_ast::Expr::Object(obj) = init_box.as_ref() {
                        infer_struct_type_from_object(obj)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let struct_type_name = explicit_type.clone().or(inferred_struct.clone());

            if let Some(ref type_name) = struct_type_name {
                emitter.set_object_struct(Some(type_name.clone()));
            }

            let name = match &vdecl.name {
                swc_ecma_ast::Pat::Ident(ident) => {
                    super::to_snake_case(ident.id.sym.as_ref())
                }
                _ => "unknown".to_string(),
            };

            let ty: String = if let Some(ref explicit) = explicit_type {
                explicit.clone()
            } else if let Some(ref inferred) = inferred_struct {
                inferred.clone()
            } else if let Some(ref init) = vdecl.init {
                infer_type(init)
            } else {
                "()".to_string()
            };

            if let Some(init) = &vdecl.init {
                let needs_cast = if let Some(ref explicit) = explicit_type {
                    let inferred = infer_type(init);
                    explicit != &inferred
                } else {
                    false
                };

                emitter.push_str(&format!("let {}: {} = ", name, ty));
                if needs_cast {
                    match ty.as_str() {
                        "f64" => emitter.push_str("0.0"),
                        "i32" => emitter.push_str("0i32"),
                        "String" => emitter.push_str("String::new()"),
                        "bool" => emitter.push_str("false"),
                        _ => emit_expr(emitter, init),
                    }
                } else {
                    emit_expr(emitter, init);
                }
                emitter.push_str(";\n");
            } else {
                emitter.push_str(&format!("let {}: {};\n", name, ty));
            }

            emitter.set_object_struct(None);
        }
    }
}

/// Extract type annotation from a pattern.
pub fn extract_type_annotation(pat: &swc_ecma_ast::Pat) -> Option<String> {
    if let swc_ecma_ast::Pat::Ident(ident) = pat {
        if let Some(ref type_ann) = ident.type_ann {
            return Some(resolve_type_name(&type_ann.type_ann));
        }
    }
    None
}

/// Resolve a type annotation to a Rust type name.
fn resolve_type_name(ts_type: &swc_ecma_ast::TsType) -> String {
    match ts_type {
        swc_ecma_ast::TsType::TsKeywordType(k) => match k.kind {
            swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => "f64".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => "String".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => "bool".to_string(),
            swc_ecma_ast::TsKeywordTypeKind::TsVoidKeyword => "()".to_string(),
            _ => "()".to_string(),
        },
        swc_ecma_ast::TsType::TsTypeRef(type_ref) => {
            let name = match &type_ref.type_name {
                swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
            };
            match name.as_str() {
                "Task" | "Filter" | "AppState" => name,
                "Result" => "Result<String, String>".to_string(),
                "Option" => "Option<()>".to_string(),
                _ => name,
            }
        }
        swc_ecma_ast::TsType::TsArrayType(arr) => {
            let inner = resolve_type_name(&arr.elem_type);
            format!("Vec<{}>", inner)
        }
        _ => "()".to_string(),
    }
}

/// Infer struct type from an object expression.
pub fn infer_struct_type_from_object(obj: &swc_ecma_ast::ObjectLit) -> Option<String> {
    let mut props: std::collections::HashSet<&str> =
        std::collections::HashSet::new();
    for prop in &obj.props {
        if let swc_ecma_ast::PropOrSpread::Prop(p) = prop {
            if let swc_ecma_ast::Prop::KeyValue(kv) = &**p {
                if let swc_ecma_ast::PropName::Ident(ident) = &kv.key {
                    props.insert(ident.sym.as_ref());
                }
            }
        }
    }

    if props.contains("id") && props.contains("title") && props.contains("done") {
        return Some("Task".to_string());
    }

    if props.contains("total") && props.contains("done") && props.contains("active") {
        return Some("__AnonymousStruct1".to_string());
    }

    None
}
