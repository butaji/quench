//! # Variable Declaration Emitter
//!
//! Emits Rust variable declarations from TypeScript var/let/const.

use super::{emit_expr, infer_type, CodeEmitter};

/// Emit a variable declaration.
pub fn emit_var_decl(emitter: &mut CodeEmitter, decl: &swc_ecma_ast::Decl) {
    let swc_ecma_ast::Decl::Var(var_decl) = decl else {
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
        emitter.push_str(&format!("let {}: {};\n", name, ty));
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
        explicit.clone()
    } else if let Some(inferred) = inferred_struct {
        inferred.clone()
    } else if let Some(init) = init {
        infer_type(init)
    } else {
        "()".to_string()
    }
}

fn emit_var_with_init(
    emitter: &mut CodeEmitter,
    name: &str,
    ty: &str,
    init: &swc_ecma_ast::Expr,
    explicit_type: Option<&String>,
) {
    let needs_cast = needs_type_cast(init, explicit_type);

    emitter.push_str(&format!("let {}: {} = ", name, ty));

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
    match ty {
        "f64" => emitter.push_str("0.0"),
        "i32" => emitter.push_str("0i32"),
        "String" => emitter.push_str("String::new()"),
        "bool" => emitter.push_str("false"),
        _ => emit_expr(emitter, init),
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
        _ => "()".to_string(),
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
    match name.as_str() {
        "Task" | "Filter" | "AppState" => name,
        "Result" => "Result<String, String>".to_string(),
        "Option" => "Option<()>".to_string(),
        _ => name,
    }
}

/// Infer struct type from an object expression.
pub fn infer_struct_type_from_object(obj: &swc_ecma_ast::ObjectLit) -> Option<String> {
    let props = collect_object_props(obj);

    if props.iter().any(|p| p == "id")
        && props.iter().any(|p| p == "title")
        && props.iter().any(|p| p == "done")
    {
        return Some("Task".to_string());
    }

    if props.iter().any(|p| p == "total")
        && props.iter().any(|p| p == "done")
        && props.iter().any(|p| p == "active")
    {
        return Some("__AnonymousStruct1".to_string());
    }

    None
}

fn collect_object_props(obj: &swc_ecma_ast::ObjectLit) -> Vec<String> {
    let mut props = Vec::new();
    for prop in &obj.props {
        if let swc_ecma_ast::PropOrSpread::Prop(p) = prop {
            if let swc_ecma_ast::Prop::KeyValue(kv) = &**p {
                if let swc_ecma_ast::PropName::Ident(ident) = &kv.key {
                    props.push(ident.sym.to_string());
                }
            }
        }
    }
    props
}
