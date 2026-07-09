//! Quench TSX/TS Compiler using native swc parsing
//!
//! Pipeline:
//! 1. swc parses TypeScript and strips type annotations
//! 2. Transform for Ink compatibility
//! 3. Execute directly via quench-runtime

use anyhow::{Context, Result};
use quench_runtime::ast::*;
use std::path::Path;

/// Compile TSX/TS source to Quench-compatible JavaScript
pub fn compile_tsx(source: &str, _filename: &str) -> Result<String> {
    compile_with_swc(source)
}

/// Compile TS/JS source
pub fn compile_ts(source: &str, _filename: &str) -> Result<String> {
    compile_with_swc(source)
}

fn compile_with_swc(source: &str) -> Result<String> {
    // Use quench-runtime to parse TypeScript and strip types
    let _ctx = quench_runtime::Context::new()
        .map_err(|e| anyhow::anyhow!("Failed to create runtime: {}", e))?;
    
    // Parse TypeScript with swc (strips type annotations)
    let program = quench_runtime::swc_parse::parse_typescript(source)
        .map_err(|e| anyhow::anyhow!("TypeScript parse error: {}", e))?;
    
    // Transform JSX to ink.createElement calls
    let transformed = transform_jsx_in_ast(&program);
    
    // Serialize AST back to JavaScript string for compatibility
    Ok(ast_to_js(&transformed))
}

/// Transform JSX elements in AST to ink.createElement calls
fn transform_jsx_in_ast(program: &quench_runtime::ast::Program) -> quench_runtime::ast::Program {
    match program {
        Program::Script(stmts) => Program::Script(transform_statements(stmts)),
    }
}

fn transform_statements(stmts: &[Statement]) -> Vec<Statement> {
    stmts.iter().map(|s| transform_statement(s)).collect()
}

fn transform_statement(stmt: &Statement) -> Statement {
    match stmt {
        Statement::Expression(expr) => Statement::Expression(Box::new(transform_expression(expr))),
        Statement::VarDeclaration { kind, name, init } => {
            Statement::VarDeclaration { 
                kind: *kind, 
                name: name.clone(), 
                init: init.as_ref().map(|e| transform_expression(e)),
            }
        }
        Statement::FunctionDeclaration { name, params, body } => Statement::FunctionDeclaration {
            name: name.clone(),
            params: params.clone(),
            body: transform_statements(body),
        },
        Statement::If { condition, consequent, alternate } => Statement::If {
            condition: Box::new(transform_expression(condition)),
            consequent: Box::new(transform_statement(consequent)),
            alternate: alternate.as_ref().map(|a| Box::new(transform_statement(a))),
        },
        Statement::While { condition, body } => Statement::While {
            condition: Box::new(transform_expression(condition)),
            body: Box::new(transform_statement(body)),
        },
        Statement::For { init, condition, update, body } => Statement::For {
            init: init.as_ref().map(|i| match i {
                ForInit::Expression(e) => ForInit::Expression(Box::new(transform_expression(e))),
                ForInit::VarDeclaration { kind, name, init } => ForInit::VarDeclaration {
                    kind: *kind,
                    name: name.clone(),
                    init: init.as_ref().map(|e| transform_expression(e)),
                },
            }),
            condition: condition.as_ref().map(|e| Box::new(transform_expression(e))),
            update: update.as_ref().map(|e| Box::new(transform_expression(e))),
            body: Box::new(transform_statement(body)),
        },
        Statement::Block(stmts) => Statement::Block(transform_statements(stmts)),
        Statement::Return(expr) => Statement::Return(expr.as_ref().map(|e| Box::new(transform_expression(e)))),
        Statement::TryCatch { body, param, handler } => Statement::TryCatch {
            body: Box::new(transform_statement(body)),
            param: param.clone(),
            handler: Box::new(transform_statement(handler)),
        },
        Statement::Throw(expr) => Statement::Throw(Box::new(transform_expression(expr))),
        _ => stmt.clone(),
    }
}

fn transform_expression(expr: &Expression) -> Expression {
    match expr {
        Expression::JsxElement { tag, props, children } => {
            // Transform to: ink.createElement(tag, props, ...children)
            let create_element = Expression::Member {
                object: Box::new(Expression::Identifier("ink".to_string())),
                property: PropertyKey::Ident("createElement".to_string()),
                computed: false,
            };
            
            // Build props object
            let prop_entries: Vec<(PropertyKey, PropertyValue)> = props.iter().filter_map(|p| {
                match p {
                    JsxProp::Attr { name, value } => {
                        let prop_value = match value {
                            JsxAttrValue::String(s) => PropertyValue::Value(Expression::String(s.clone())),
                            JsxAttrValue::Expression(e) => PropertyValue::Value(e.clone()),
                        };
                        Some((PropertyKey::Ident(name.clone()), prop_value))
                    }
                    JsxProp::Spread(_) => None, // Skip spreads
                }
            }).collect();
            let props_expr = Expression::Object(prop_entries);
            
            // Build tag string
            let tag_str = match tag {
                JsxTagName::Ident(s) => s.clone(),
                JsxTagName::Member { object, property } => format!("{}.{}", object, property),
                JsxTagName::Namespaced { namespace, name } => format!("{}:{}", namespace, name),
            };
            
            // Build arguments
            let mut args = vec![Expression::String(tag_str), props_expr];
            for child in children {
                match child {
                    JsxChild::Text(s) => {
                        // Normalize whitespace: collapse multiple spaces, trim edges
                        let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !normalized.is_empty() {
                            args.push(Expression::String(normalized));
                        }
                    }
                    JsxChild::Expression(e) => args.push(e.clone()),
                    JsxChild::Spread(_) => {}
                    JsxChild::Element(e) => args.push(transform_expression(e)),
                }
            }
            
            Expression::Call {
                callee: Box::new(create_element),
                arguments: args,
            }
        }
        Expression::JsxFragment { children } => {
            // Transform to: ink.createElement(ink.Fragment, null, ...children)
            let create_element = Expression::Member {
                object: Box::new(Expression::Identifier("ink".to_string())),
                property: PropertyKey::Ident("createElement".to_string()),
                computed: false,
            };
            let fragment_ref = Expression::Member {
                object: Box::new(Expression::Identifier("ink".to_string())),
                property: PropertyKey::Ident("Fragment".to_string()),
                computed: false,
            };
            let mut args = vec![fragment_ref, Expression::Null];
            for child in children {
                match child {
                    JsxChild::Text(s) => {
                        // Normalize whitespace: collapse multiple spaces, trim edges
                        let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !normalized.is_empty() {
                            args.push(Expression::String(normalized));
                        }
                    }
                    JsxChild::Expression(e) => args.push(e.clone()),
                    JsxChild::Spread(_) => {}
                    JsxChild::Element(e) => args.push(transform_expression(e)),
                }
            }
            Expression::Call {
                callee: Box::new(create_element),
                arguments: args,
            }
        }
        Expression::Call { callee, arguments } => Expression::Call {
            callee: Box::new(transform_expression(callee)),
            arguments: arguments.iter().map(|a| transform_expression(a)).collect(),
        },
        Expression::Member { object, property, computed } => Expression::Member {
            object: Box::new(transform_expression(object)),
            property: property.clone(),
            computed: *computed,
        },
        Expression::Binary { op, left, right } => Expression::Binary {
            op: *op,
            left: Box::new(transform_expression(left)),
            right: Box::new(transform_expression(right)),
        },
        Expression::Unary { op, argument } => Expression::Unary {
            op: *op,
            argument: Box::new(transform_expression(argument)),
        },
        Expression::Assignment { left, right } => Expression::Assignment {
            left: Box::new(transform_expression(left)),
            right: Box::new(transform_expression(right)),
        },
        Expression::CompoundAssignment { op, left, right } => Expression::CompoundAssignment {
            op: *op,
            left: Box::new(transform_expression(left)),
            right: Box::new(transform_expression(right)),
        },
        Expression::Conditional { condition, consequent, alternate } => Expression::Conditional {
            condition: Box::new(transform_expression(condition)),
            consequent: Box::new(transform_expression(consequent)),
            alternate: Box::new(transform_expression(alternate)),
        },
        Expression::Update { op, argument, prefix } => Expression::Update {
            op: *op,
            argument: Box::new(transform_expression(argument)),
            prefix: *prefix,
        },
        Expression::New { constructor, arguments } => Expression::New {
            constructor: Box::new(transform_expression(constructor)),
            arguments: arguments.iter().map(|a| transform_expression(a)).collect(),
        },
        Expression::Sequence(exprs) => Expression::Sequence(exprs.iter().map(|e| transform_expression(e)).collect()),
        Expression::FunctionExpression { name, params, body } => Expression::FunctionExpression {
            name: name.clone(),
            params: params.clone(),
            body: body.iter().map(|s| transform_statement(s)).collect(),
        },
        Expression::ArrowFunction { params, body } => Expression::ArrowFunction {
            params: params.clone(),
            body: match &**body {
                ArrowBody::Expression(e) => Box::new(ArrowBody::Expression(transform_expression(e))),
                ArrowBody::Block(stmts) => Box::new(ArrowBody::Block(std::rc::Rc::new(transform_statements(stmts)))),
            },
        },
        Expression::BlockExpr(stmts) => Expression::BlockExpr(transform_statements(stmts)),
        Expression::ForOf { variable, iterable, body } => Expression::ForOf {
            variable: Box::new(transform_expression(variable)),
            iterable: Box::new(transform_expression(iterable)),
            body: Box::new(transform_statement(body)),
        },
        Expression::ForIn { variable, object, body } => Expression::ForIn {
            variable: Box::new(transform_expression(variable)),
            object: Box::new(transform_expression(object)),
            body: Box::new(transform_statement(body)),
        },
        Expression::OptChain { object, property, computed } => Expression::OptChain {
            object: Box::new(transform_expression(object)),
            property: property.clone(),
            computed: *computed,
        },
        Expression::OptChainCall { object, property, computed, arguments } => Expression::OptChainCall {
            object: Box::new(transform_expression(object)),
            property: property.clone(),
            computed: *computed,
            arguments: arguments.iter().map(|a| transform_expression(a)).collect(),
        },
        Expression::ArrayPattern(elems) => Expression::ArrayPattern(elems.iter().map(|e| transform_binding_element(e)).collect()),
        Expression::ObjectPattern(props) => Expression::ObjectPattern(props.iter().map(|(k, v)| (k.clone(), transform_binding_element(v))).collect()),
        _ => expr.clone(),
    }
}

fn transform_binding_element(elem: &quench_runtime::ast::BindingElement) -> quench_runtime::ast::BindingElement {
    match elem {
        BindingElement::Identifier(s) => BindingElement::Identifier(s.clone()),
        BindingElement::ArrayPattern(elems) => BindingElement::ArrayPattern(elems.iter().map(|e| transform_binding_element(e)).collect()),
        BindingElement::ObjectPattern(props) => BindingElement::ObjectPattern(props.iter().map(|(k, v)| (k.clone(), transform_binding_element(v))).collect()),
    }
}

/// Convert runtime AST back to JavaScript string
/// This is a simple serialization for now - full implementation would use swc_codegen
fn ast_to_js(program: &quench_runtime::ast::Program) -> String {
    match program {
        quench_runtime::ast::Program::Script(stmts) => {
            stmts.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n")
        }
    }
}

fn stmt_to_js(stmt: &quench_runtime::ast::Statement) -> String {
    match stmt {
        Statement::Expression(expr) => format!("{};", expr_to_js(expr)),
        Statement::VarDeclaration { kind, name, init } => {
            let kw = match kind {
                VarKind::Var => "var",
                VarKind::Let => "let",
                VarKind::Const => "const",
            };
            match init {
                Some(e) => format!("{} {} = {};", kw, name, expr_to_js(e)),
                None => format!("{} {};", kw, name),
            }
        }
        Statement::FunctionDeclaration { name, params, body } => {
            let params_str = params.join(", ");
            let body_str = body.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
            format!("function {}({}) {{\n{}}}", name, params_str, body_str)
        }
        Statement::ClassDeclaration { name, class: _ } => {
            // Class declarations are not fully supported in the compiler output
            // Return a placeholder that will fail at runtime
            format!("var {} = null; // class not supported", name)
        }
        Statement::Return(Some(expr)) => format!("return {};", expr_to_js(expr)),
        Statement::Return(None) => "return;".to_string(),
        Statement::Block(stmts) => {
            let inner = stmts.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
            format!("{{ {}}}", inner)
        }
        Statement::If { condition, consequent, alternate } => {
            let cond_js = expr_to_js(condition);
            let cons_js = stmt_to_js(consequent);
            match alternate {
                Some(alt) => format!("if ({}) {} else {}", cond_js, cons_js, stmt_to_js(alt)),
                None => format!("if ({}) {}", cond_js, cons_js),
            }
        }
        Statement::While { condition, body } => {
            format!("while ({}) {}", expr_to_js(condition), stmt_to_js(body))
        }
        Statement::For { init, condition, update, body } => {
            let init_js = match init {
                Some(ForInit::Expression(e)) => expr_to_js(e),
                Some(ForInit::VarDeclaration { kind, name, init }) => {
                    let kw = match kind { VarKind::Var => "var", VarKind::Let => "let", VarKind::Const => "const" };
                    match init {
                        Some(e) => format!("{} {} = {}", kw, name, expr_to_js(e)),
                        None => format!("{} {}", kw, name),
                    }
                }
                None => "".to_string(),
            };
            let cond_js = condition.as_ref().map(|e| expr_to_js(e)).unwrap_or_default();
            let update_js = update.as_ref().map(|e| expr_to_js(e)).unwrap_or_default();
            format!("for ({}; {}; {}) {}", init_js, cond_js, update_js, stmt_to_js(body))
        }
        Statement::Empty => ";".to_string(),
        Statement::Break(label) => match label {
            Some(l) => format!("break {};", l),
            None => "break;".to_string(),
        },
        Statement::Continue(label) => match label {
            Some(l) => format!("continue {};", l),
            None => "continue;".to_string(),
        },
        Statement::Throw(expr) => format!("throw {};", expr_to_js(expr)),
        Statement::TryCatch { body, param, handler } => {
            let handler_js = format!("catch ({}) {}", param.as_deref().unwrap_or("e"), stmt_to_js(handler));
            format!("try {} {}", stmt_to_js(body), handler_js)
        }
        Statement::SequenceDecls(stmts) => stmts.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n"),
        // ES module export - compile the inner statement
        Statement::Export(inner) => stmt_to_js(inner),
        // ES module import - placeholder (not supported in inline compilation)
        Statement::Import { .. } => "// import statement not supported".to_string(),
        // For-in loop
        Statement::ForIn { variable, object, body } => {
            format!("for ({} in {}) {}", expr_to_js(variable), expr_to_js(object), stmt_to_js(body))
        }
    }
}

fn expr_to_js(expr: &quench_runtime::ast::Expression) -> String {
    match expr {
        Expression::Number(n) => n.to_string(),
        Expression::String(s) => format!("\"{}\"", s),
        Expression::Boolean(b) => b.to_string(),
        Expression::Null => "null".to_string(),
        Expression::Undefined => "undefined".to_string(),
        Expression::Identifier(name) => name.clone(),
        Expression::Array(elems) => {
            let inner = elems.iter().map(|e| {
                match e {
                    Expression::Spread(inner) => format!("...{}", expr_to_js(inner)),
                    _ => expr_to_js(e),
                }
            }).collect::<Vec<_>>().join(", ");
            format!("[{}]", inner)
        }
        Expression::Object(props) => {
            let inner = props.iter().map(|(k, v)| {
                let k_str = match k {
                    PropertyKey::Ident(n) => n.clone(),
                    PropertyKey::String(s) => format!("\"{}\"", s),
                    PropertyKey::Number(n) => n.to_string(),
                    PropertyKey::Computed(e) => format!("[{}]", expr_to_js(e)),
                };
                match v {
                    PropertyValue::Value(e) => format!("{}: {}", k_str, expr_to_js(e)),
                    PropertyValue::Getter { params: _, body } => {
                        let body_str = body.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
                        format!("get {}() {{ {} }}", k_str, body_str)
                    }
                    PropertyValue::Setter { param, body } => {
                        let body_str = body.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
                        format!("set {}({}) {{ {} }}", k_str, param, body_str)
                    }
                }
            }).collect::<Vec<_>>().join(", ");
            format!("{{{}}}", inner)
        }
        Expression::Binary { op, left, right } => {
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Mod => "%",
                BinaryOp::And => "&&",
                BinaryOp::Or => "||",
                BinaryOp::Eq => "==",
                BinaryOp::Neq => "!=",
                BinaryOp::StrictEq => "===",
                BinaryOp::StrictNeq => "!==",
                BinaryOp::Lt => "<",
                BinaryOp::Gt => ">",
                BinaryOp::Le => "<=",
                BinaryOp::Ge => ">=",
                BinaryOp::BitAnd => "&",
                BinaryOp::BitOr => "|",
                BinaryOp::BitXor => "^",
                BinaryOp::Shl => "<<",
                BinaryOp::Shr => ">>",
                BinaryOp::Ushr => ">>>",
                BinaryOp::In => "in",
                BinaryOp::Instanceof => "instanceof",
                BinaryOp::NullishCoalescing => "??",
            };
            format!("({} {} {})", expr_to_js(left), op_str, expr_to_js(right))
        }
        Expression::Unary { op, argument } => {
            let op_str = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
                UnaryOp::Plus => "+",
                UnaryOp::BitNot => "~",
                UnaryOp::Typeof => "typeof",
                UnaryOp::Void => "void",
            };
            format!("{} {}", op_str, expr_to_js(argument))
        }
        Expression::Assignment { left, right } => {
            format!("{} = {}", expr_to_js(left), expr_to_js(right))
        }
        Expression::CompoundAssignment { op, left, right } => {
            let op_str = match op {
                CompoundOp::Add => "+=",
                CompoundOp::Sub => "-=",
                CompoundOp::Mul => "*=",
                CompoundOp::Div => "/=",
                CompoundOp::Mod => "%=",
                CompoundOp::BitAnd => "&=",
                CompoundOp::BitOr => "|=",
                CompoundOp::BitXor => "^=",
                CompoundOp::Shl => "<<=",
                CompoundOp::Shr => ">>=",
                CompoundOp::Ushr => ">>>=",
            };
            format!("{} {} {}", expr_to_js(left), op_str, expr_to_js(right))
        }
        Expression::Call { callee, arguments } => {
            let args_str = arguments.iter().map(expr_to_js).collect::<Vec<_>>().join(", ");
            format!("{}({})", expr_to_js(callee), args_str)
        }
        Expression::Member { object, property, computed } => {
            let obj_str = expr_to_js(object);
            let prop_str = match property {
                PropertyKey::Ident(n) if !computed => n.clone(),
                PropertyKey::Ident(n) => format!("[{}]", n),
                PropertyKey::String(s) if *computed => format!("[\"{}\"]", s),
                PropertyKey::String(s) => s.clone(),
                PropertyKey::Number(n) => n.to_string(),
                PropertyKey::Computed(e) => format!("[{}]", expr_to_js(e)),
            };
            if *computed {
                format!("({})[{}]", obj_str, prop_str)
            } else {
                format!("({}).{}", obj_str, prop_str)
            }
        }
        Expression::Conditional { condition, consequent, alternate } => {
            format!("{} ? {} : {}", expr_to_js(condition), expr_to_js(consequent), expr_to_js(alternate))
        }
        Expression::Update { op, argument, prefix } => {
            let op_str = match op { UpdateOp::Increment => "++", UpdateOp::Decrement => "--" };
            if *prefix {
                format!("{}{}", op_str, expr_to_js(argument))
            } else {
                format!("{}{}", expr_to_js(argument), op_str)
            }
        }
        Expression::New { constructor, arguments } => {
            let args_str = arguments.iter().map(expr_to_js).collect::<Vec<_>>().join(", ");
            format!("new {}({})", expr_to_js(constructor), args_str)
        }
        Expression::Sequence(exprs) => {
            exprs.iter().map(expr_to_js).collect::<Vec<_>>().join(", ")
        }
        Expression::FunctionExpression { name, params, body } => {
            let name_str = name.as_ref().map(|n| format!(" {} ", n)).unwrap_or_default();
            let params_str = params.join(", ");
            let body_str = body.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
            format!("function{}({}) {{ {} }}", name_str, params_str, body_str)
        }
        Expression::ArrowFunction { params, body } => {
            let params_str = params.join(", ");
            match &**body {
                ArrowBody::Expression(e) => {
                    let expr_js = expr_to_js(e);
                    // Object literals need parentheses: () => ({...})
                    let wrapped = if expr_js.starts_with('{') {
                        format!("({})", expr_js)
                    } else {
                        expr_js
                    };
                    format!("({}) => {}", params_str, wrapped)
                }
                ArrowBody::Block(stmts) => {
                    let body_str = stmts.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
                    format!("({}) => {{ {} }}", params_str, body_str)
                }
            }
        }
        Expression::BlockExpr(stmts) => {
            let body_str = stmts.iter().map(stmt_to_js).collect::<Vec<_>>().join("\n");
            format!("{{ {} }}", body_str)
        }
        Expression::ForOf { variable, iterable, body } => {
            format!("for ({} of {}) {}", expr_to_js(variable), expr_to_js(iterable), stmt_to_js(body))
        }
        Expression::ForIn { variable, object, body } => {
            format!("for ({} in {}) {}", expr_to_js(variable), expr_to_js(object), stmt_to_js(body))
        }
        Expression::OptChain { object, property, computed } => {
            format!("{}.?{}", expr_to_js(object), expr_to_js(&Expression::Member { object: Box::new((**object).clone()), property: property.clone(), computed: *computed }))
        }
        Expression::OptChainCall { object, property, computed, arguments } => {
            let args_str = arguments.iter().map(expr_to_js).collect::<Vec<_>>().join(", ");
            format!("{}.?{}({})", expr_to_js(object), expr_to_js(&Expression::Member { object: Box::new((**object).clone()), property: property.clone(), computed: *computed }), args_str)
        }
        Expression::JsxElement { tag, props, children } => {
            let tag_str = match tag {
                JsxTagName::Ident(s) => s.clone(),
                JsxTagName::Member { object, property } => format!("{}.{}", object, property),
                JsxTagName::Namespaced { namespace, name } => format!("{}:{}", namespace, name),
            };
            let props_str = if props.is_empty() {
                "".to_string()
            } else {
                let inner = props.iter().map(|p| {
                    match p {
                        JsxProp::Attr { name, value } => {
                            match value {
                                JsxAttrValue::String(s) => format!(" {}=\"{}\"" , name, s),
                                JsxAttrValue::Expression(e) => {
                                    let e_str = expr_to_js(e);
                                    // Boolean shorthand: bold={true} -> bold, bold={false} -> omitted
                                    if e_str == "true" {
                                        return format!(" {}", name);
                                    } else if e_str == "false" {
                                        return String::new(); // Omit false boolean props
                                    } else {
                                        let val = format!("{{{}}}", e_str);
                                        return format!(" {}={}", name, val);
                                    }
                                }
                            }
                        }
                        JsxProp::Spread(e) => format!(" {{...{}}}", expr_to_js(e)),
                    }
                }).collect::<Vec<_>>().join("");
                inner
            };
            let children_str = if children.is_empty() {
                "".to_string()
            } else {
                let inner = children.iter().map(|c| {
                    match c {
                        JsxChild::Text(s) => s.clone(),
                        JsxChild::Expression(e) => format!("{{ {} }}", expr_to_js(e)),
                        JsxChild::Spread(e) => format!("{{...{}}}", expr_to_js(e)),
                        JsxChild::Element(e) => expr_to_js(e),
                    }
                }).collect::<Vec<_>>().join("");
                inner
            };
            if children_str.is_empty() {
                format!("<{}{} />", tag_str, props_str)
            } else {
                format!("<{}{}>{}</{}>", tag_str, props_str, children_str, tag_str)
            }
        }
        Expression::JsxFragment { children } => {
            let children_str = children.iter().map(|c| {
                match c {
                    JsxChild::Text(s) => s.clone(),
                    JsxChild::Expression(e) => format!("{{ {} }}", expr_to_js(e)),
                    JsxChild::Spread(e) => format!("{{...{}}}", expr_to_js(e)),
                    JsxChild::Element(e) => expr_to_js(e),
                }
            }).collect::<Vec<_>>().join(" ");
            format!("<>{}", children_str)
        }
        Expression::ArrayPattern(_) | Expression::ObjectPattern(_) => {
            // Destructuring patterns - simplified representation
            "[]".to_string()
        }
        Expression::Class(_) => {
            // Class expressions are not supported in the compiler output
            "null".to_string()
        }
        Expression::Spread(inner) => {
            format!("...{}", expr_to_js(inner))
        }
    }
}

fn extract_ink_imports(js: &str) -> Vec<String> {
    js.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("import ") && (trimmed.contains("from \"ink\"") || trimmed.contains("from 'ink'"))
        })
        .flat_map(extract_names_from_import)
        .collect()
}

fn extract_names_from_import(line: &str) -> Vec<String> {
    let start = match line.find('{') {
        Some(i) => i + 1,
        None => return vec![],
    };
    let end = match line.find('}') {
        Some(i) => i,
        None => return vec![],
    };
    line[start..end]
        .split(',')
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && n != "React")
        .collect()
}

fn prefix_components(js: &str, imports: &[String]) -> String {
    let mut result = js.to_string();
    for name in imports {
        // Protect already-prefixed
        result = result.replace(&format!("ink.ink.{}", name), &format!("ink.{}", name));
        // Prefix in createElement calls
        let from = format!("ink.createElement({}", name);
        let to = format!("ink.createElement(ink.{}", name);
        result = result.replace(&from, &to);
    }
    // Clean up
    result.replace("ink.ink.", "ink.")
}

fn prefix_hooks(js: &str) -> String {
    // All React hooks + Ink hooks that can be called as functions
    let hooks = [
        // React hooks
        "useState", "useEffect", "useRef", "useMemo", "useCallback",
        "useContext", "useReducer", "useLayoutEffect", "useImperativeHandle",
        "useDebugValue",
        // Ink hooks
        "useInput", "useApp", "useStdin", "useStdout", "useStderr",
        "useFocus", "useFocusManager", "useWindowSize", "useAnimation",
        "usePaste", "useCursor", "useBoxMetrics", "useIsScreenReaderEnabled",
        "measureElement", "useBridge", "createContext",
    ];
    let mut result = js.to_string();
    for hook in &hooks {
        // Protect already-prefixed
        result = result.replace(&format!("ink.ink.{}", hook), &format!("ink.{}", hook));
        // Prefix call sites
        result = result.replace(hook, &format!("ink.{}", hook));
    }
    result.replace("ink.ink.", "ink.")
}

fn strip_imports(js: &str) -> String {
    js.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                let from_react = trimmed.contains("from \"react\"") || trimmed.contains("from 'react'");
                let from_ink = trimmed.contains("from \"ink\"") || trimmed.contains("from 'ink'");
                return !(from_react || from_ink);
            }
            if trimmed.starts_with("import type ") {
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace CommonJS require() variable references with global ink/react.
/// esbuild --format=cjs outputs: var import_ink = require("ink");
/// This function strips the require line and replaces all import_ink.X with ink.X
fn replace_ink_require_refs(js: &str) -> String {
    let mut result = String::new();
    let mut ink_var_name: Option<String> = None;
    let mut react_var_name: Option<String> = None;
    
    for line in js.lines() {
        let trimmed = line.trim();
        
        // Detect ink require: var import_ink = require("ink");
        if trimmed.starts_with("var ") && trimmed.contains("require(") {
            if trimmed.contains("\"ink\"") || trimmed.contains("'ink'") {
                // Extract variable name: "var import_ink = require..." -> "import_ink"
                if let Some(var_name) = extract_cjs_var_name(trimmed) {
                    ink_var_name = Some(var_name);
                    continue; // Skip the require line
                }
            }
            if trimmed.contains("\"react\"") || trimmed.contains("'react'") {
                if let Some(var_name) = extract_cjs_var_name(trimmed) {
                    react_var_name = Some(var_name);
                    continue; // Skip the require line
                }
            }
        }
        
        result.push_str(line);
        result.push('\n');
    }
    
    // Replace references to ink variable with ink global
    if let Some(ref var_name) = ink_var_name {
        result = result.replace(&format!("{}.", var_name), "ink.");
        result = result.replace(&format!("{}", var_name), "ink");
    }
    
    // For React, just remove references (we convert React.createElement to ink.createElement)
    if let Some(ref var_name) = react_var_name {
        // Replace React.createElement calls - the var name becomes irrelevant
        result = result.replace(&format!("{}.createElement(", var_name), "React.createElement(");
    }
    
    result
}

/// Extract the variable name from a CJS require line.
/// "var import_ink = require(\"ink\")" -> Some("import_ink")
fn extract_cjs_var_name(line: &str) -> Option<String> {
    // Pattern: "var " followed by identifier followed by " = require"
    let after_var = line.strip_prefix("var ")?;
    let before_require = after_var.split("=").next()?;
    Some(before_require.trim().to_string())
}

// SHIMS constant
static SHIMS: &str = r#"// Quench Node.js/React shims
const __tb_keypress_handlers = [];
const __tb_readline_shim = {
    createInterface: (options) => {
        const inputObj = {
            _handlers: {},
            on: function(evt, handler) {
                if (evt === 'keypress') {
                    this._handlers.keypress = this._handlers.keypress || [];
                    this._handlers.keypress.push(handler);
                    __tb_keypress_handlers.push(handler);
                }
            },
            removeListener: function(evt, handler) {
                if (evt === 'keypress' && this._handlers.keypress) {
                    this._handlers.keypress = this._handlers.keypress.filter(h => h !== handler);
                    __tb_keypress_handlers = __tb_keypress_handlers.filter(h => h !== handler);
                }
            },
        };
        return {
            input: inputObj,
            output: { write: () => {} },
            question: (q, cb) => { if (typeof cb === 'function') cb(''); },
            on: () => {},
            close: () => {},
        };
    },
};
const __tb_readline_key_aliases = {
    upArrow: 'up', downArrow: 'down', leftArrow: 'left', rightArrow: 'right',
    pageUp: 'pageup', pageDown: 'pagedown',
    return: 'return', escape: 'escape', tab: 'tab', backspace: 'backspace',
    delete: 'delete', home: 'home', end: 'end', insert: 'insert',
};
function __tb_to_readline_name(name) {
    if (!name) return name;
    if (Object.prototype.hasOwnProperty.call(__tb_readline_key_aliases, name)) {
        return __tb_readline_key_aliases[name];
    }
    if (/^f\d+$/.test(name)) return name;
    return name;
}
// Readline shim: override __tb_dispatch_key to also call readline handlers
// The runtime.js version reads from globals, so we need to hook into that flow
(function() {
    const __tb_orig_dispatch_key = globalThis.__tb_dispatch_key;
    globalThis.__tb_dispatch_key = function() {
        // Call the original (reads globals, calls inputHandlers)
        if (__tb_orig_dispatch_key) __tb_orig_dispatch_key();
        
        // Also call readline handlers
        const key = globalThis.__pending_key;
        const ctrl = globalThis.__pending_ctrl;
        const shift = globalThis.__pending_shift;
        const alt = globalThis.__pending_alt;
        const meta = globalThis.__pending_meta;
        const name = __tb_to_readline_name(key);
        const str = name.length === 1 ? name : '';
        const keyObj = { ctrl: ctrl, meta: meta, shift: shift, name: name, sequence: name.length === 1 ? name : '' };
        for (const handler of __tb_keypress_handlers) {
            try { 
                handler(str, keyObj); 
            } catch(e) { 
                console.error('[shim] Handler error:', e); 
            }
        }
    };
})();
const __tb_import_sync = function(moduleName) {
    const isStr = typeof moduleName === 'string';
    const name = isStr ? moduleName.replace('node:', '') : '';
    if (name === 'readline') { return __tb_readline_shim; }
    return {};
};
globalThis.__tb_import = function(moduleName) {
    const result = __tb_import_sync(moduleName);
    return { then: function(onFulfilled) { onFulfilled(result); }, catch: function() { return this; } };
};
globalThis.process = {
    exit: (code) => { try { ink.useApp().exit(); } catch(e) {} },
    stdout: {
      write: (s) => { try { ink.stdout_write(s); } catch(e) {} },
      get rows() { try { return ink.useStdout().rows; } catch(e) { return 24; } },
      get columns() { try { return ink.useStdout().columns; } catch(e) { return 80; } },
      isTTY: true
    },
    stderr: { write: (s) => { try { ink.stderr_write(s); } catch(e) {} } },
    stdin: {
      isTTY: true,
      setRawMode: () => {},
      resume: () => {},
      on: () => {},
      removeListener: () => {},
    },
    env: { NODE_ENV: 'production' },
    on: (evt, cb) => {},
};

"#;

fn prepend_shims(js: &str) -> String {
    let mut result = js.to_string();
    result.insert_str(0, SHIMS);
    result
}

/// Compile a file
pub fn compile_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {:?}", path))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.tsx");

    if filename.ends_with(".tsx") || filename.ends_with(".jsx") {
        compile_tsx(&source, filename)
    } else {
        compile_ts(&source, filename)
    }
}
