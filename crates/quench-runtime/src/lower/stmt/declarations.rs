//! Declaration lowering functions

use crate::ast::{Class, ClassMember, Expression, Param, PropertyKey, Statement, VarKind};
use oxc::ast::ast;

use super::lower_stmt;
use crate::lower::expr::lower_expr;

/// Lower a declaration (function, var, const, let, class)
pub fn lower_decl(decl: &ast::Declaration) -> Option<Statement> {
    match decl {
        ast::Declaration::FunctionDeclaration(func_decl) => lower_fn_decl(func_decl),
        ast::Declaration::VariableDeclaration(var_decl) => lower_var_decl(var_decl),
        ast::Declaration::ClassDeclaration(class_decl) => lower_class_decl(class_decl),
        _ => None,
    }
}

#[allow(clippy::complexity)]
pub fn lower_var_decl(var_decl: &ast::VariableDeclaration) -> Option<Statement> {
    lower_var_decl_impl(var_decl, None)
}

/// Lower a variable declaration, using `iterable_override` as the init expression for
/// destructuring patterns when present (used in for-of/for-in loops).
/// For `for (let [a, b] of iterable)`, the iterable is passed here so the pattern
/// can access elements from it rather than from a separate initializer.
#[allow(clippy::complexity)]
pub fn lower_var_decl_impl(
    var_decl: &ast::VariableDeclaration,
    iterable_override: Option<Expression>,
) -> Option<Statement> {
    use crate::lower::stmt::destructuring::{
        lower_array_destructuring, lower_object_destructuring, wrap_decls,
    };

    let kind = match var_decl.kind {
        ast::VariableDeclarationKind::Var => VarKind::Var,
        ast::VariableDeclarationKind::Let => VarKind::Let,
        ast::VariableDeclarationKind::Const => VarKind::Const,
        // Using/AwaitUsing not supported
        ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing => {
            return None;
        }
    };
    let mut decls = Vec::new();
    for binding in &var_decl.declarations {
        let ident_name = match &binding.id.kind {
            ast::BindingPatternKind::BindingIdentifier(ident) => {
                Some(ident.name.as_str().to_string())
            }
            _ => None,
        };
        let init_expr = iterable_override.clone().or_else(|| {
            binding.init.as_ref().and_then(|e| {
                let mut lowered = lower_expr(e).ok()?;
                // Per ES §14.6.13 step 18a, if the initializer is an
                // anonymous class expression, the inferred name is the
                // binding identifier. We set the name on the lowerer-produced
                // `Expression::Class` so the static-field initializer can
                // observe it via `this.name`.
                if let Some(name) = &ident_name {
                    if let crate::ast::Expression::Class(class) = &mut lowered {
                        if class.name.is_none() {
                            class.name = Some(name.clone());
                        }
                    }
                }
                Some(lowered)
            })
        });
        match &binding.id.kind {
            ast::BindingPatternKind::BindingIdentifier(ident) => {
                decls.push(Statement::VarDeclaration {
                    kind,
                    name: ident.name.as_str().to_string(),
                    init: init_expr,
                });
            }
            ast::BindingPatternKind::ArrayPattern(arr) => {
                decls.extend(lower_array_destructuring(kind, arr, init_expr, decls.len()));
            }
            ast::BindingPatternKind::ObjectPattern(obj) => {
                decls.extend(lower_object_destructuring(
                    kind,
                    obj,
                    init_expr,
                    decls.len(),
                ));
            }
            ast::BindingPatternKind::AssignmentPattern(assign) => {
                // `[a = default] = init` → `let a = init["0"] ?? default`
                let ident_name = match &assign.left.kind {
                    ast::BindingPatternKind::BindingIdentifier(id) => id.name.as_str().to_string(),
                    _ => continue, // Nested patterns not yet supported here
                };
                if let Some(init) = init_expr {
                    // Build: init[0] ?? default
                    let accessor = Expression::Member {
                        object: Box::new(init.clone()),
                        property: PropertyKey::Number(0.0),
                        computed: true,
                    };
                    let default_expr = crate::lower::expr::lower_expr(&assign.right)
                        .ok()
                        .unwrap_or(Expression::Undefined);
                    let initializer = Expression::Binary {
                        left: Box::new(accessor),
                        op: crate::ast::BinaryOp::NullishCoalescing,
                        right: Box::new(default_expr),
                    };
                    decls.push(Statement::VarDeclaration {
                        kind,
                        name: ident_name,
                        init: Some(initializer),
                    });
                }
            }
        }
    }
    wrap_decls(decls)
}

pub fn lower_fn_decl(func_decl: &ast::Function) -> Option<Statement> {
    let name = func_decl.id.as_ref().map(|i| i.name.as_str().to_string())?;
    let params = lower_formal_params(&func_decl.params);
    let body = func_decl
        .body
        .as_ref()
        .map(|b| {
            let mut stmts: Vec<Statement> = b.statements.iter().filter_map(lower_stmt).collect();
            // Add directives (e.g. "use strict") before statements so
            // eval-time check_use_strict can find them.
            // Insert in reverse order so the first directive ends up at index 0.
            // Only include directives whose raw text is a string literal (no escape
            // sequences or line continuations, per ES §14.1.1).
            for d in b.directives.iter().rev() {
                let raw_opt = d.expression.raw.as_ref().map(|a| a.to_string());
                // Per ES §14.1.1, a Use Strict Directive must be exactly
                // "use strict" or 'use strict' (no escape sequences, no line
                // continuations). OXC's value field resolves escapes, so we
                // check the raw source text instead.
                let is_use_strict = raw_opt.as_deref()
                    .map(|r| r == "\"use strict\"" || r == "'use strict'")
                    .unwrap_or(false);
                if is_use_strict {
                    stmts.insert(
                        0,
                        Statement::Expression(Box::new(Expression::String(
                            d.expression.value.to_string(),
                        ))),
                    );
                }
            }
            stmts
        })
        .unwrap_or_default();
    Some(Statement::FunctionDeclaration {
        name,
        params,
        body,
        is_async: func_decl.r#async,
        is_generator: func_decl.generator,
    })
}

/// Lower a single FormalParameter to Param
pub fn lower_param_decl(param: &ast::FormalParameter) -> Param {
    lower_binding_pattern(&param.pattern)
}

/// Lower a BindingPattern to Param (used for both params and destructuring)
pub fn lower_binding_pattern(binding: &ast::BindingPattern) -> Param {
    match &binding.kind {
        ast::BindingPatternKind::BindingIdentifier(ident) => Param::new(ident.name.as_str()),
        ast::BindingPatternKind::AssignmentPattern(assign) => {
            let default = lower_expr(&assign.right).ok().map(Box::new);
            match &assign.left.kind {
                ast::BindingPatternKind::BindingIdentifier(ident) => Param {
                    name: ident.name.as_str().to_string(),
                    default,
                    pattern: None,
                    rest: false,
                },
                _ => Param {
                    name: "arg".to_string(),
                    default: None,
                    pattern: crate::lower::pattern::lower_binding_elem(binding).ok(),
                    rest: false,
                },
            }
        }
        _ => Param {
            name: "arg".to_string(),
            default: None,
            pattern: crate::lower::pattern::lower_binding_elem(binding).ok(),
            rest: false,
        },
    }
}

/// Lower FormalParameters (items + rest) to Vec<Param>
pub fn lower_formal_params(params: &ast::FormalParameters) -> Vec<Param> {
    let mut result: Vec<Param> = params.items.iter().map(lower_param_decl).collect();
    // Handle rest parameter: stored separately in FormalParameters.rest
    if let Some(rest) = &params.rest {
        let mut param = match crate::lower::pattern::lower_binding_elem(&rest.argument) {
            Ok(crate::ast::BindingElement::Identifier(name)) => Param::rest(&name),
            Ok(pattern) => Param {
                name: "arg".to_string(),
                default: None,
                pattern: Some(pattern),
                rest: true,
            },
            Err(_) => Param::rest("arg"),
        };
        param.rest = true;
        result.push(param);
    }
    result
}

pub fn lower_class_decl(class_decl: &ast::Class) -> Option<Statement> {
    let name = class_decl
        .id
        .as_ref()
        .map(|i| i.name.as_str().to_string())?;
    let class = lower_class(class_decl)?;
    Some(Statement::ClassDeclaration { name, class })
}

pub fn lower_class(class: &ast::Class) -> Option<Class> {
    // Class name is not stored in the Class struct, only in ClassDecl
    let name: Option<String> = None;
    let super_class = class.super_class.as_ref().and_then(|e| lower_expr(e).ok());
    let body: Vec<ClassMember> = class
        .body
        .body
        .iter()
        .filter_map(lower_class_member_stmt)
        .collect();
    Some(Class {
        name,
        super_class: super_class.map(Box::new),
        body,
    })
}

fn lower_class_member_stmt(member: &ast::ClassElement) -> Option<ClassMember> {
    match member {
        ast::ClassElement::MethodDefinition(method) => {
            // Check if this method is a constructor
            if method.kind == ast::MethodDefinitionKind::Constructor {
                lower_constructor_stmt(method)
            } else {
                lower_method_stmt(method)
            }
        }
        ast::ClassElement::PropertyDefinition(prop) => lower_class_prop_stmt(prop),
        _ => None,
    }
}

fn lower_constructor_stmt(method: &ast::MethodDefinition) -> Option<ClassMember> {
    let ps: Vec<String> = method
        .value
        .params
        .items
        .iter()
        .filter_map(|p| match &p.pattern.kind {
            ast::BindingPatternKind::BindingIdentifier(ident) => {
                Some(ident.name.as_str().to_string())
            }
            _ => None,
        })
        .collect();
    let body = method
        .value
        .body
        .as_ref()
        .map(|b| b.statements.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Some(ClassMember::Constructor { params: ps, body })
}

#[allow(clippy::complexity)]
fn lower_method_stmt(method: &ast::MethodDefinition) -> Option<ClassMember> {
    let name = lower_prop_name_stmt(&method.key)?;
    let is_static = method.r#static;
    let ps: Vec<Param> = lower_formal_params(&method.value.params);
    let body = method
        .value
        .body
        .as_ref()
        .map(|b| b.statements.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    match method.kind {
        ast::MethodDefinitionKind::Get => Some(ClassMember::Getter { name, body }),
        ast::MethodDefinitionKind::Set => {
            let param = ps.first().map(|p| p.name.clone()).unwrap_or_default();
            Some(ClassMember::Setter { name, param, body })
        }
        _ => {
            if is_static {
                Some(ClassMember::StaticMethod {
                    name,
                    params: ps,
                    body,
                })
            } else {
                Some(ClassMember::Method {
                    name,
                    params: ps,
                    body,
                })
            }
        }
    }
}

fn lower_class_prop_stmt(prop: &ast::PropertyDefinition) -> Option<ClassMember> {
    let name = lower_prop_name_stmt(&prop.key)?;
    let value = match &prop.value {
        Some(expr) => lower_expr(expr).ok()?,
        None => Expression::Undefined,
    };
    if prop.r#static {
        Some(ClassMember::StaticField { name, value })
    } else {
        Some(ClassMember::Field { name, value })
    }
}

pub fn lower_prop_name_stmt(key: &ast::PropertyKey) -> Option<PropertyKey> {
    match key {
        ast::PropertyKey::StaticIdentifier(i) => {
            Some(PropertyKey::Ident(i.name.as_str().to_string()))
        }
        ast::PropertyKey::PrivateIdentifier(i) => Some(PropertyKey::Ident(format!("#{}", i.name))),
        ast::PropertyKey::StringLiteral(s) => Some(PropertyKey::String(s.value.to_string())),
        ast::PropertyKey::NumericLiteral(n) => Some(PropertyKey::Number(n.value)),
        ast::PropertyKey::BigIntLiteral(b) => Some(PropertyKey::String(format!("{}n", b.raw))),
        ast::PropertyKey::BooleanLiteral(b) => Some(PropertyKey::String(b.value.to_string())),
        ast::PropertyKey::NullLiteral(_) => Some(PropertyKey::String("null".to_string())),
        ast::PropertyKey::TemplateLiteral(_) => None,
        // In OXC, computed property names are Expression variants in PropertyKey
        // These require runtime evaluation and can't be handled as static keys
        _ => None,
    }
}
