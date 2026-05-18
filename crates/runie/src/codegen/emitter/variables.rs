//! # Variable Declaration Emitter
//!
//! Emits Rust variable declarations from TypeScript var/let/const.

use super::{emit_expr, infer_type, infer::UNKNOWN_TYPE, CodeEmitter};

/// Emit a variable declaration.
pub fn emit_var_decl(emitter: &mut CodeEmitter, decl: &swc_ecma_ast::Decl) {

    let swc_ecma_ast::Decl::Var(var_decl) = decl else {
        eprintln!("DEBUG emit_var_decl: not a Var decl");
        return;
    };

    for decl in &var_decl.decls {
        emit_single_var_decl(emitter, decl);
    }
}

fn emit_single_var_decl(emitter: &mut CodeEmitter, decl: &swc_ecma_ast::VarDeclarator) {

    let explicit_type = extract_type_annotation(&decl.name);

    let inferred_struct = infer_struct_from_init(&decl.name, decl.init.as_deref());

    let struct_type_name = explicit_type.clone().or(inferred_struct.clone());

    if let Some(ref type_name) = struct_type_name {
        emitter.set_object_struct(Some(type_name.clone()));
    }

    let name = extract_var_name(&decl.name);
    let ty = resolve_var_type(
        explicit_type.as_ref(),
        inferred_struct.as_ref(),
        decl.init.as_deref(),
    );

    if let Some(init) = &decl.init {
        emit_var_with_init(emitter, &name, &ty, init, explicit_type.as_ref());
    } else {
        // Only emit type annotation if we have one
        if ty != UNKNOWN_TYPE {
            emitter.push_str(&format!("let {}: {};\n", name, ty));
        } else {
            emitter.push_str(&format!("let {};\n", name));
        }
    }

    emitter.set_object_struct(None);
}

fn infer_struct_from_init(
    name: &swc_ecma_ast::Pat,
    init: Option<&swc_ecma_ast::Expr>,
) -> Option<String> {
    if extract_type_annotation(name).is_some() {
        return None;
    }
    if let Some(swc_ecma_ast::Expr::Object(obj)) = init {
        return infer_struct_type_from_object(obj);
    }
    None
}

fn extract_var_name(name: &swc_ecma_ast::Pat) -> String {
    if let swc_ecma_ast::Pat::Ident(ident) = name {
        super::to_snake_case(ident.id.sym.as_ref())
    } else {
        "unknown".to_string()
    }
}

fn resolve_var_type(
    explicit_type: Option<&String>,
    inferred_struct: Option<&String>,
    init: Option<&swc_ecma_ast::Expr>,
) -> String {
    if let Some(explicit) = explicit_type {
        return explicit.clone();
    }
    if let Some(inferred) = inferred_struct {
        return inferred.clone();
    }
    if let Some(init_expr) = init {
        let inferred = infer_type(init_expr);
        // Don't use UNKNOWN_TYPE as default - return known type or UNKNOWN
        return inferred;
    }

    UNKNOWN_TYPE.to_string()
}

fn emit_var_with_init(
    emitter: &mut CodeEmitter,
    name: &str,
    ty: &str,
    init: &swc_ecma_ast::Expr,
    explicit_type: Option<&String>,
) {
    let needs_cast = needs_type_cast(init, explicit_type);

    // Only emit type annotation if we have a known type
    if ty != UNKNOWN_TYPE {
        emitter.push_str(&format!("let {}: {} = ", name, ty));
    } else {
        emitter.push_str(&format!("let {} = ", name));
    }

    if needs_cast {
        emit_with_cast(emitter, ty, init);
    } else {
        emit_expr(emitter, init);
    }

    emitter.push_str(";\n");
}

fn needs_type_cast(init: &swc_ecma_ast::Expr, explicit_type: Option<&String>) -> bool {
    if let Some(explicit) = explicit_type {
        let inferred = infer_type(init);
        explicit != &inferred
    } else {
        false
    }
}

fn emit_with_cast(emitter: &mut CodeEmitter, ty: &str, init: &swc_ecma_ast::Expr) {
    emit_expr(emitter, init);
    match ty {
        "f64" => emitter.push_str(" as f64"),
        "i32" => emitter.push_str(" as i32"),
        "usize" => emitter.push_str(" as usize"),
        "String" => emitter.push_str(".to_string()"),
        "bool" => emitter.push_str(" as bool"),
        _ => {}
    }
}

/// Extract type annotation from a pattern.
pub fn extract_type_annotation(pat: &swc_ecma_ast::Pat) -> Option<String> {
    let swc_ecma_ast::Pat::Ident(ident) = pat else {
        return None;
    };
    let type_ann = ident.type_ann.as_ref()?;
    Some(resolve_type_name(&type_ann.type_ann))
}

/// Resolve a type annotation to a Rust type name.
fn resolve_type_name(ts_type: &swc_ecma_ast::TsType) -> String {
    match ts_type {
        swc_ecma_ast::TsType::TsKeywordType(k) => resolve_keyword_type(k.kind),
        swc_ecma_ast::TsType::TsTypeRef(type_ref) => resolve_type_ref(type_ref),
        swc_ecma_ast::TsType::TsArrayType(arr) => {
            let inner = resolve_type_name(&arr.elem_type);
            format!("Vec<{}>", inner)
        }
        swc_ecma_ast::TsType::TsUnionOrIntersectionType(union) => {
            resolve_union_type(union)
        }
        _ => "()".to_string(),
    }
}

/// Resolve a union type like `number | null` to `Option<f64>`.
fn resolve_union_type(union: &swc_ecma_ast::TsUnionOrIntersectionType) -> String {
    let swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u) = union else {
        return "()".to_string();
    };

    // Check for Option pattern: T | null
    if u.types.len() == 2 {
        let has_null = u.types.iter().any(|t| is_null_type(t));
        if has_null {
            let non_null = u.types.iter().find(|t| !is_null_type(t));
            if let Some(t) = non_null {
                let inner = resolve_type_name(t.as_ref());
                return format!("Option<{}>", inner);
            }
        }
    }

    "()".to_string()
}

/// Check if a type is the null keyword type.
fn is_null_type(ts_type: &swc_ecma_ast::TsType) -> bool {
    if let swc_ecma_ast::TsType::TsKeywordType(k) = ts_type {
        k.kind == swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword
    } else {
        false
    }
}

fn resolve_keyword_type(kind: swc_ecma_ast::TsKeywordTypeKind) -> String {
    match kind {
        swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => "f64".to_string(),
        swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => "String".to_string(),
        swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => "bool".to_string(),
        swc_ecma_ast::TsKeywordTypeKind::TsVoidKeyword => "()".to_string(),
        _ => "()".to_string(),
    }
}

fn resolve_type_ref(type_ref: &swc_ecma_ast::TsTypeRef) -> String {
    let name = match &type_ref.type_name {
        swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
        swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
    };

    // Handle generic types with parameters
    if let Some(params) = &type_ref.type_params {
        return resolve_generic_type_ref(&name, params);
    }

    match name.as_str() {
        "Result" => "Result<(), ()>".to_string(),
        "Option" => "Option<()>".to_string(),
        "Record" => "std::collections::HashMap<String, ()>".to_string(),
        _ => name,
    }
}

fn resolve_generic_type_ref(
    name: &str,
    params: &swc_ecma_ast::TsTypeParamInstantiation,
) -> String {
    let params_str: Vec<String> = params
        .params
        .iter()
        .map(|p| resolve_type_name(p))
        .collect();

    match name {
        "Record" | "Map" => {
            if params_str.len() >= 2 {
                format!(
                    "std::collections::HashMap<{}, {}>",
                    params_str[0], params_str[1]
                )
            } else {
                "std::collections::HashMap<String, ()>".to_string()
            }
        }
        "Set" => {
            if !params_str.is_empty() {
                format!("std::collections::HashSet<{}>", params_str[0])
            } else {
                "std::collections::HashSet<()>".to_string()
            }
        }
        "Array" | "Vec" => {
            if !params_str.is_empty() {
                format!("Vec<{}>", params_str[0])
            } else {
                "Vec<()>".to_string()
            }
        }
        "Option" => {
            if !params_str.is_empty() {
                format!("Option<{}>", params_str[0])
            } else {
                "Option<()>".to_string()
            }
        }
        "Result" => {
            if !params_str.is_empty() {
                format!("Result<{}, String>", params_str[0])
            } else {
                "Result<(), String>".to_string()
            }
        }
        _ => {
            if params_str.is_empty() {
                name.to_string()
            } else {
                params_str.join(", ")
            }
        }
    }
}

/// Infer struct type from an object expression.
pub fn infer_struct_type_from_object(_obj: &swc_ecma_ast::ObjectLit) -> Option<String> {
    None
}


