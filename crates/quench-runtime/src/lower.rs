//! Lower SWC AST to runtime AST
//!
//! Converts swc_ecma_ast nodes to our runtime AST representation.

use swc_ecma_ast as swc;
use swc_atoms::Atom;
use crate::ast::*;
use crate::ast::Program;

/// Error during lowering
#[derive(Debug, Clone)]
pub struct LowerError {
    pub message: String,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LowerError {}

impl LowerError {
    pub fn new(message: impl Into<String>) -> Self {
        LowerError { message: message.into() }
    }
}

/// Convert Atom to String using Display trait
fn atom_to_string(atom: &Atom) -> String {
    atom.to_string()
}

/// Convert Wtf8Atom to String using Display trait
fn wtf8_atom_to_string(atom: &swc_atoms::Wtf8Atom) -> String {
    atom.to_string_lossy().into_owned()
}

/// Lower a binding pattern (for destructuring) to BindingElement
#[allow(dead_code)]
fn lower_binding_elem(pat: &swc::Pat) -> Result<BindingElement, LowerError> {
    match pat {
        swc::Pat::Ident(ident) => Ok(BindingElement::Identifier(atom_to_string(&ident.id.sym))),
        swc::Pat::Array(arr) => {
            let elements: Vec<BindingElement> = arr.elems.iter()
                .filter_map(|e| {
                    match e {
                        Some(elem) => {
                            let elem_ref: &swc::Pat = elem;
                            match elem_ref {
                                swc::Pat::Ident(id) => Some(BindingElement::Identifier(atom_to_string(&id.id.sym))),
                                swc::Pat::Array(arr) => lower_binding_elem(&swc::Pat::Array(arr.clone())).ok(),
                                swc::Pat::Object(obj) => lower_binding_elem(&swc::Pat::Object(obj.clone())).ok(),
                                swc::Pat::Rest(rest) => lower_binding_elem(&rest.arg).ok(),
                                swc::Pat::Assign(assign) => lower_binding_elem(&assign.left).ok(),
                                _ => None,
                            }
                        }
                        None => Some(BindingElement::Identifier("__hole".to_string())), // Sparse array element
                    }
                })
                .collect();
            Ok(BindingElement::ArrayPattern(elements))
        }
        swc::Pat::Object(obj) => {
            let props: Vec<(PropertyKey, BindingElement)> = obj.props.iter()
                .filter_map(|prop| {
                    match prop {
                        swc::ObjectPatProp::KeyValue(kv) => {
                            let key = match &kv.key {
                                swc::PropName::Ident(i) => PropertyKey::Ident(atom_to_string(&i.sym)),
                                swc::PropName::Str(s) => PropertyKey::String(wtf8_atom_to_string(&s.value)),
                                swc::PropName::Num(n) => PropertyKey::Number(n.value),
                                swc::PropName::Computed(_) => PropertyKey::String("computed".to_string()),
                                swc::PropName::BigInt(_) => PropertyKey::String("bigint".to_string()),
                            };
                            lower_binding_elem(&kv.value).ok().map(|e| (key, e))
                        }
                        swc::ObjectPatProp::Rest(_) => {
                            // Skip rest pattern in destructuring for now
                            None
                        }
                        swc::ObjectPatProp::Assign(assign) => {
                            // Handle default value assignment: { a = 5 }
                            // assign.value is Option<Box<Expr>>, not a Pat
                            // For now, just use the key as the variable name
                            let key = PropertyKey::Ident(atom_to_string(&assign.key.sym));
                            Some((key, BindingElement::Identifier(atom_to_string(&assign.key.sym))))
                        }
                    }
                })
                .collect();
            Ok(BindingElement::ObjectPattern(props))
        }
        swc::Pat::Rest(rest) => lower_binding_elem(&rest.arg),
        swc::Pat::Assign(assign) => lower_binding_elem(&assign.left),
        _ => Err(LowerError::new("Unsupported binding pattern")),
    }
}

/// Lower a binding pattern to extract variable names for scope
/// (Simplified version - main logic is in VarDeclaration handling)
#[allow(dead_code)]
fn binding_to_vars(_pat: &swc::Pat, _init_expr: &Option<swc::Expr>) -> Vec<(String, Option<Expression>)> {
    // This function is kept for potential future use
    // The main destructuring expansion is handled in the VarDeclaration case
    vec![]
}

/// Expand a nested binding pattern into variable declarations
fn expand_nested_pattern(kind: VarKind, pat: &swc::Pat, source_var: &str) -> Vec<Statement> {
    let source = Expression::Identifier(source_var.to_string());
    match pat {
        swc::Pat::Ident(ident) => {
            vec![Statement::VarDeclaration {
                kind,
                name: atom_to_string(&ident.id.sym),
                init: Some(source),
            }]
        }
        swc::Pat::Array(arr) => expand_nested_array_pattern(kind, arr, source_var),
        swc::Pat::Object(obj) => expand_nested_object_pattern(kind, obj, source_var),
        _ => vec![],
    }
}

/// Expand array pattern: [a, b] from source_var
fn expand_nested_array_pattern(kind: VarKind, arr: &swc::ArrayPat, source_var: &str) -> Vec<Statement> {
    let mut stmts = Vec::new();
    for (i, elem) in arr.elems.iter().enumerate() {
        match elem {
            Some(elem) => {
                let elem_ref: &swc::Pat = elem;
                match elem_ref {
                    swc::Pat::Ident(id) => {
                        let name = atom_to_string(&id.id.sym);
                        let member = Expression::Member {
                            object: Box::new(Expression::Identifier(source_var.to_string())),
                            property: PropertyKey::Number(i as f64),
                            computed: true,
                        };
                        stmts.push(Statement::VarDeclaration {
                            kind,
                            name,
                            init: Some(member),
                        });
                    }
                    _ => {
                        let temp_name = format!("{}_item_{}", source_var, i);
                        let member = Expression::Member {
                            object: Box::new(Expression::Identifier(source_var.to_string())),
                            property: PropertyKey::Number(i as f64),
                            computed: true,
                        };
                        stmts.push(Statement::VarDeclaration {
                            kind,
                            name: temp_name.clone(),
                            init: Some(member),
                        });
                        stmts.extend(expand_nested_pattern(kind, elem_ref, &temp_name));
                    }
                }
            }
            None => { /* sparse element */ }
        }
    }
    stmts
}

/// Expand object pattern: {a, b} from source_var
fn expand_nested_object_pattern(kind: VarKind, obj: &swc::ObjectPat, source_var: &str) -> Vec<Statement> {
    let mut stmts = Vec::new();
    for prop in &obj.props {
        match prop {
            swc::ObjectPatProp::KeyValue(kv) => {
                let key_str = match &kv.key {
                    swc::PropName::Ident(i) => atom_to_string(&i.sym),
                    swc::PropName::Str(s) => wtf8_atom_to_string(&s.value),
                    swc::PropName::Num(n) => n.value.to_string(),
                    _ => continue,
                };
                if key_str.is_empty() { continue; }
                
                let kv_value_ref: &swc::Pat = &kv.value;
                let var_name = match kv_value_ref {
                    swc::Pat::Ident(id) => atom_to_string(&id.id.sym),
                    _ => format!("{}_prop_{}", source_var, key_str),
                };
                
                let member = Expression::Member {
                    object: Box::new(Expression::Identifier(source_var.to_string())),
                    property: PropertyKey::String(key_str.clone()),
                    computed: false,
                };
                
                match kv_value_ref {
                    swc::Pat::Ident(_) => {
                        stmts.push(Statement::VarDeclaration {
                            kind,
                            name: var_name,
                            init: Some(member),
                        });
                    }
                    swc::Pat::Object(nested_obj) => {
                        let temp_name = format!("{}_prop_{}", source_var, key_str);
                        stmts.push(Statement::VarDeclaration {
                            kind,
                            name: temp_name.clone(),
                            init: Some(member),
                        });
                        stmts.extend(expand_nested_object_pattern(kind, nested_obj, &temp_name));
                    }
                    swc::Pat::Array(nested_arr) => {
                        let temp_name = format!("{}_prop_{}", source_var, key_str);
                        stmts.push(Statement::VarDeclaration {
                            kind,
                            name: temp_name.clone(),
                            init: Some(member),
                        });
                        stmts.extend(expand_nested_array_pattern(kind, nested_arr, &temp_name));
                    }
                    _ => {
                        stmts.push(Statement::VarDeclaration {
                            kind,
                            name: var_name,
                            init: Some(member),
                        });
                    }
                }
            }
            swc::ObjectPatProp::Assign(assign) => {
                let var_name = atom_to_string(&assign.key.sym);
                let member = Expression::Member {
                    object: Box::new(Expression::Identifier(source_var.to_string())),
                    property: PropertyKey::Ident(var_name.clone()),
                    computed: false,
                };
                stmts.push(Statement::VarDeclaration {
                    kind,
                    name: var_name,
                    init: Some(member),
                });
            }
            swc::ObjectPatProp::Rest(_) => { /* Skip rest */ }
        }
    }
    stmts
}

/// Lower a swc Module to our runtime Program
pub fn lower_module(module: &swc::Module) -> Result<Program, LowerError> {
    let statements: Vec<Statement> = module.body.iter()
        .filter_map(lower_module_item)
        .collect();
    Ok(Program::Script(statements))
}

/// Lower a swc Script to our runtime Program
pub fn lower_script(script: &swc::Script) -> Result<Program, LowerError> {
    let statements: Vec<Statement> = script.body.iter()
        .filter_map(lower_stmt)
        .collect();
    Ok(Program::Script(statements))
}

/// Lower a swc ModuleItem to a Statement
fn lower_module_item(item: &swc::ModuleItem) -> Option<Statement> {
    match item {
        swc::ModuleItem::Stmt(stmt) => lower_stmt(stmt),
        swc::ModuleItem::ModuleDecl(decl) => lower_module_decl(decl),
    }
}

fn lower_module_decl(decl: &swc::ModuleDecl) -> Option<Statement> {
    match decl {
        swc::ModuleDecl::ExportDefaultDecl(_) => None,
        swc::ModuleDecl::ExportDefaultExpr(expr) => {
            Some(Statement::Expression(Box::new(lower_expr(&expr.expr).ok()?)))
        }
        _ => None,
    }
}

/// Lower a swc Stmt to our Statement
#[allow(unreachable_patterns)]
fn lower_stmt(stmt: &swc::Stmt) -> Option<Statement> {
    match stmt {
        swc::Stmt::Empty(_) => Some(Statement::Empty),
        swc::Stmt::Block(block) => {
            let stmts: Vec<Statement> = block.stmts.iter().filter_map(lower_stmt).collect();
            Some(Statement::Block(stmts))
        }
        swc::Stmt::Break(_) => Some(Statement::Break(None)),
        swc::Stmt::Continue(_) => Some(Statement::Continue(None)),
        swc::Stmt::Debugger(_) => Some(Statement::Empty),
        swc::Stmt::With(_) => None,
        swc::Stmt::Decl(decl) => lower_decl(decl),
        swc::Stmt::Return(ret) => {
            let expr = ret.arg.as_ref().and_then(|e| lower_expr(e).ok());
            Some(Statement::Return(expr.map(Box::new)))
        }
        swc::Stmt::Labeled(labeled) => lower_stmt(&labeled.body),
        swc::Stmt::If(if_stmt) => {
            let condition = lower_expr(&if_stmt.test).ok()?;
            let consequent = Box::new(lower_stmt(&if_stmt.cons).unwrap_or(Statement::Empty));
            let alternate = if_stmt.alt.as_ref().map(|a| {
                Box::new(lower_stmt(a).unwrap_or(Statement::Empty))
            });
            Some(Statement::If {
                condition: Box::new(condition),
                consequent,
                alternate,
            })
        }
        swc::Stmt::Switch(switch) => lower_switch(switch),
        swc::Stmt::Throw(throw) => {
            let expr = lower_expr(&throw.arg).ok()?;
            Some(Statement::Throw(Box::new(expr)))
        }
        swc::Stmt::Try(try_stmt) => {
            let body = Box::new(lower_stmt(&swc::Stmt::Block(try_stmt.block.clone())).unwrap_or(Statement::Empty));
            
            // Extract catch parameter name
            let catch_param = try_stmt.handler.as_ref().and_then(|catch| {
                catch.param.as_ref().and_then(|pat| {
                    match pat {
                        swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                        _ => None,
                    }
                })
            });
            
            let handler = if let Some(catch) = &try_stmt.handler {
                Box::new(lower_stmt(&swc::Stmt::Block(catch.body.clone())).unwrap_or(Statement::Empty))
            } else {
                Box::new(Statement::Empty)
            };
            Some(Statement::TryCatch { body, param: catch_param, handler })
        }
        swc::Stmt::While(while_stmt) => {
            let condition = lower_expr(&while_stmt.test).ok()?;
            let body = Box::new(lower_stmt(&while_stmt.body).unwrap_or(Statement::Empty));
            Some(Statement::While { condition: Box::new(condition), body })
        }
        swc::Stmt::DoWhile(_) => None,
        swc::Stmt::For(for_stmt) => {
            let init = for_stmt.init.as_ref().and_then(lower_for_init);
            let condition = for_stmt.test.as_ref().and_then(|e| lower_expr(e).ok()).map(Box::new);
            let update = for_stmt.update.as_ref().and_then(|e| lower_expr(e).ok()).map(Box::new);
            let body = Box::new(lower_stmt(&for_stmt.body).unwrap_or(Statement::Empty));
            Some(Statement::For { init, condition, update, body })
        }
        swc::Stmt::ForIn(for_in_stmt) => {
            // Lower the left-hand side
            let left = lower_for_lhs(&for_in_stmt.left)?;
            let iterable = lower_expr(&for_in_stmt.right).ok()?;
            let body = Box::new(lower_stmt(&for_in_stmt.body).unwrap_or(Statement::Empty));
            Some(Statement::Expression(Box::new(Expression::ForIn {
                variable: Box::new(left),
                object: Box::new(iterable),
                body,
            })))
        }
        swc::Stmt::ForOf(for_of_stmt) => {
            // Lower the left-hand side
            let left = lower_for_lhs(&for_of_stmt.left)?;
            let iterable = lower_expr(&for_of_stmt.right).ok()?;
            let body = Box::new(lower_stmt(&for_of_stmt.body).unwrap_or(Statement::Empty));
            Some(Statement::Expression(Box::new(Expression::ForOf {
                variable: Box::new(left),
                iterable: Box::new(iterable),
                body,
            })))
        }
        swc::Stmt::Expr(expr_stmt) => {
            // Expression statement: 1 + 2; -> Expression(1 + 2)
            let expr = lower_expr(&expr_stmt.expr).ok()?;
            Some(Statement::Expression(Box::new(expr)))
        }
        // Other statement types we don't support yet
        _ => None,
    }
}

/// Lower a declaration (function, var, const, let, class)
fn lower_decl(decl: &swc::Decl) -> Option<Statement> {
    match decl {
        swc::Decl::Fn(func_decl) => {
            let name = func_decl.ident.sym.to_string();
            let params = func_decl.function.params.iter().map(|p| {
                // Try to extract param name from pattern
                match &p.pat {
                    swc::Pat::Ident(ident) => ident.id.sym.to_string(),
                    _ => "".to_string(),
                }
            }).collect();
            let body = func_decl.function.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Some(Statement::FunctionDeclaration { name, params, body })
        }
        swc::Decl::Var(var_decl) => {
            // Handle var declarations - process ALL bindings including destructuring
            let kind = match var_decl.kind {
                swc::VarDeclKind::Var => VarKind::Var,
                swc::VarDeclKind::Let => VarKind::Let,
                swc::VarDeclKind::Const => VarKind::Const,
            };
            
            let mut decls = Vec::new();
            for binding in &var_decl.decls {
                // Get the initializer expression
                let init_expr = binding.init.as_ref().and_then(|e| lower_expr(e).ok());
                
                match &binding.name {
                    swc::Pat::Ident(ident) => {
                        // Simple identifier
                        let name = ident.id.sym.to_string();
                        decls.push(Statement::VarDeclaration {
                            kind,
                            name,
                            init: init_expr,
                        });
                    }
                    swc::Pat::Array(arr) => {
                        // Array destructuring: [a, b] = expr
                        // Use a temp variable to ensure the source expression is evaluated once
                        let mut stmts = Vec::new();
                        
                        // Create a temp variable for the source expression
                        let temp_var_name = format!("__arr_src_{}", decls.len());
                        stmts.push(Statement::VarDeclaration {
                            kind: VarKind::Const, // Temp is always const
                            name: temp_var_name.clone(),
                            init: init_expr.clone(),
                        });
                        
                        // Create member accesses from the temp variable
                        for (i, elem) in arr.elems.iter().enumerate() {
                            match elem {
                                Some(elem) => {
                                    let elem_ref: &swc::Pat = elem;
                                    match elem_ref {
                                        swc::Pat::Ident(id) => {
                                            let name = atom_to_string(&id.id.sym);
                                            // Use computed=false with Number property for constant indices
                                            let member = Expression::Member {
                                                object: Box::new(Expression::Identifier(temp_var_name.clone())),
                                                property: PropertyKey::Number(i as f64),
                                                computed: false,
                                            };
                                            stmts.push(Statement::VarDeclaration {
                                                kind,
                                                name,
                                                init: Some(member),
                                            });
                                        }
                                        _ => {
                                            // Nested pattern - create a temp for this element
                                            let elem_temp_name = format!("__arr_elem_{}", i);
                                            let member = Expression::Member {
                                                object: Box::new(Expression::Identifier(temp_var_name.clone())),
                                                property: PropertyKey::Number(i as f64),
                                                computed: false,
                                            };
                                            stmts.push(Statement::VarDeclaration {
                                                kind: VarKind::Const,
                                                name: elem_temp_name.clone(),
                                                init: Some(member),
                                            });
                                            // Then handle the nested pattern
                                            let nested_stmts = expand_nested_pattern(kind, elem_ref, &elem_temp_name);
                                            stmts.extend(nested_stmts);
                                        }
                                    }
                                }
                                None => {
                                    // Sparse array element - skip
                                }
                            }
                        }
                        decls.push(Statement::Block(stmts));
                    }
                    swc::Pat::Object(obj) => {
                        // Object destructuring: {a, b} = expr
                        // Use a temp variable to ensure the source expression is evaluated once
                        let mut stmts = Vec::new();
                        
                        // Create a temp variable for the source expression
                        let temp_var_name = format!("__obj_src_{}", decls.len());
                        stmts.push(Statement::VarDeclaration {
                            kind: VarKind::Const, // Temp is always const
                            name: temp_var_name.clone(),
                            init: init_expr.clone(),
                        });
                        
                        for prop in &obj.props {
                            match prop {
                                swc::ObjectPatProp::KeyValue(kv) => {
                                    let key_str = match &kv.key {
                                        swc::PropName::Ident(i) => atom_to_string(&i.sym),
                                        swc::PropName::Str(s) => wtf8_atom_to_string(&s.value),
                                        swc::PropName::Num(n) => n.value.to_string(),
                                        _ => continue,
                                    };
                                    if key_str.is_empty() { continue; }
                                    
                                    let kv_value_ref: &swc::Pat = &kv.value;
                                    let var_name = match kv_value_ref {
                                        swc::Pat::Ident(id) => atom_to_string(&id.id.sym),
                                        _ => format!("__obj_temp_{}", key_str),
                                    };
                                    
                                    let member = Expression::Member {
                                        object: Box::new(Expression::Identifier(temp_var_name.clone())),
                                        property: PropertyKey::String(key_str.clone()),
                                        computed: false,
                                    };
                                    
                                    match kv_value_ref {
                                        swc::Pat::Ident(_) => {
                                            stmts.push(Statement::VarDeclaration {
                                                kind,
                                                name: var_name,
                                                init: Some(member),
                                            });
                                        }
                                        swc::Pat::Object(nested_obj) => {
                                            // Create temp and recurse
                                            let nested_temp_name = format!("__obj_prop_{}", key_str);
                                            stmts.push(Statement::VarDeclaration {
                                                kind: VarKind::Const,
                                                name: nested_temp_name.clone(),
                                                init: Some(member),
                                            });
                                            let nested_stmts = expand_nested_object_pattern(kind, nested_obj, &nested_temp_name);
                                            stmts.extend(nested_stmts);
                                        }
                                        swc::Pat::Array(nested_arr) => {
                                            let nested_temp_name = format!("__obj_prop_{}", key_str);
                                            stmts.push(Statement::VarDeclaration {
                                                kind: VarKind::Const,
                                                name: nested_temp_name.clone(),
                                                init: Some(member),
                                            });
                                            let nested_stmts = expand_nested_array_pattern(kind, nested_arr, &nested_temp_name);
                                            stmts.extend(nested_stmts);
                                        }
                                        _ => {
                                            stmts.push(Statement::VarDeclaration {
                                                kind,
                                                name: var_name,
                                                init: Some(member),
                                            });
                                        }
                                    }
                                }
                                swc::ObjectPatProp::Assign(assign) => {
                                    // Handle default: { a = 5 }
                                    let var_name = atom_to_string(&assign.key.sym);
                                    let member = Expression::Member {
                                        object: Box::new(Expression::Identifier(temp_var_name.clone())),
                                        property: PropertyKey::Ident(var_name.clone()),
                                        computed: false,
                                    };
                                    // TODO: Handle default value properly
                                    stmts.push(Statement::VarDeclaration {
                                        kind,
                                        name: var_name,
                                        init: Some(member),
                                    });
                                }
                                swc::ObjectPatProp::Rest(_) => {
                                    // Skip rest pattern for now
                                }
                            }
                        }
                        decls.push(Statement::Block(stmts));
                    }
                    _ => continue,
                }
            }
            
            // Handle the declarations - wrap everything in a Block to ensure sequential evaluation
            // This ensures all declarations (including destructuring) are evaluated in order
            if decls.is_empty() {
                Some(Statement::Empty)
            } else if decls.len() == 1 {
                Some(decls.into_iter().next().unwrap())
            } else {
                // Multiple declarations - wrap in a block
                Some(Statement::Block(decls))
            }
        }
        swc::Decl::TsInterface(_) | swc::Decl::TsTypeAlias(_) | 
        swc::Decl::TsEnum(_) | swc::Decl::TsModule(_) => None,
        _ => None,
    }
}

fn lower_switch(switch: &swc::SwitchStmt) -> Option<Statement> {
    let discriminant = lower_expr(&switch.discriminant).ok()?;
    
    // Build nested if-else from switch cases
    let mut current: Option<Statement> = None;
    
    for case in switch.cases.iter().rev() {
        let case_body = case.cons.iter()
            .filter_map(lower_stmt)
            .collect::<Vec<_>>();
        
        let new_stmt = if let Some(test) = &case.test {
            let test_expr = lower_expr(test).ok()?;
            Statement::If {
                condition: Box::new(Expression::Binary {
                    op: BinaryOp::StrictEq,
                    left: Box::new(discriminant.clone()),
                    right: Box::new(test_expr),
                }),
                consequent: Box::new(Statement::Block(case_body)),
                alternate: current.map(Box::new),
            }
        } else {
            // Default case
            Statement::Block(case_body)
        };
        current = Some(new_stmt);
    }
    
    current.or(Some(Statement::Empty))
}

fn lower_for_init(init: &swc::VarDeclOrExpr) -> Option<ForInit> {
    match init {
        swc::VarDeclOrExpr::VarDecl(decl) => {
            let first = decl.decls.first()?;
            let kind = match decl.kind {
                swc::VarDeclKind::Var => VarKind::Var,
                swc::VarDeclKind::Let => VarKind::Let,
                swc::VarDeclKind::Const => VarKind::Const,
            };
            let name = match &first.name {
                swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                _ => return None,
            };
            let init = first.init.as_ref().and_then(|e| lower_expr(e).ok());
            Some(ForInit::VarDeclaration { kind, name, init })
        }
        swc::VarDeclOrExpr::Expr(expr) => {
            Some(ForInit::Expression(Box::new(lower_expr(expr).ok()?)))
        }
    }
}

/// Lower the left-hand side of a for-in/for-of loop
fn lower_for_lhs(left: &swc::ForHead) -> Option<Expression> {
    match left {
        swc::ForHead::VarDecl(decl) => {
            // for (var x of iterable) or for (let x of iterable)
            let first = decl.decls.first()?;
            match &first.name {
                swc::Pat::Ident(ident) => {
                    let name = atom_to_string(&ident.id.sym);
                    // Return the identifier; the interpreter will handle declaration
                    Some(Expression::Identifier(name))
                }
                _ => None,
            }
        }
        swc::ForHead::Pat(pat) => {
            // for (x of iterable) - no declaration
            match pat.as_ref() {
                swc::Pat::Ident(ident) => {
                    Some(Expression::Identifier(atom_to_string(&ident.id.sym)))
                }
                _ => None, // Destructuring in for-of not supported yet
            }
        }
        swc::ForHead::UsingDecl(_) => None,
    }
}

/// Lower a swc Expr to our Expression
fn lower_expr(expr: &swc::Expr) -> Result<Expression, LowerError> {
    match expr {
        swc::Expr::Ident(ident) => Ok(Expression::Identifier(atom_to_string(&ident.sym))),
        swc::Expr::This(_) => Ok(Expression::Identifier("this".to_string())),
        swc::Expr::Array(arr) => {
            let elements: Vec<Expression> = arr.elems.iter()
                .filter_map(|e| e.as_ref().and_then(|e| lower_expr(&e.expr).ok()))
                .collect();
            Ok(Expression::Array(elements))
        }
        swc::Expr::Object(obj) => {
            let props: Vec<(PropertyKey, PropertyValue)> = obj.props.iter()
                .filter_map(|prop| lower_prop_or_spread(prop).ok())
                .collect();
            Ok(Expression::Object(props))
        }
        swc::Expr::Fn(func) => {
            let name = func.ident.as_ref().map(|i| atom_to_string(&i.sym));
            let params = func.function.params.iter().map(|p| {
                match &p.pat {
                    swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                    _ => "arg".to_string(),
                }
            }).collect();
            let body = func.function.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Ok(Expression::FunctionExpression { name, params, body })
        }
        swc::Expr::Arrow(arrow) => {
            // ArrowExpr.params is Vec<Pat>, not Vec<Param>
            let params: Vec<String> = arrow.params.iter().map(|p| {
                match p {
                    swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                    _ => "arg".to_string(),
                }
            }).collect();
            
            let body = match arrow.body.as_ref() {
                swc::BlockStmtOrExpr::BlockStmt(block) => {
                    ArrowBody::Block(std::rc::Rc::new(block.stmts.iter().filter_map(lower_stmt).collect()))
                }
                swc::BlockStmtOrExpr::Expr(expr) => {
                    ArrowBody::Expression(lower_expr(expr)?)
                }
            };
            Ok(Expression::ArrowFunction { params, body: Box::new(body) })
        }
        swc::Expr::Yield(yield_expr) => {
            if yield_expr.delegate {
                return Err(LowerError::new("Yield delegate not supported"));
            }
            let arg = yield_expr.arg.as_ref().map(|e| lower_expr(e)).transpose()?;
            Ok(arg.unwrap_or(Expression::Undefined))
        }
        swc::Expr::MetaProp(_) => Ok(Expression::Undefined),
        swc::Expr::Await(await_expr) => {
            let arg = lower_expr(&await_expr.arg)?;
            Ok(arg) // Convert await to its argument for sync execution
        }
        swc::Expr::Paren(paren) => lower_expr(&paren.expr),
        swc::Expr::Bin(bin) => {
            let left = lower_expr(&bin.left)?;
            let right = lower_expr(&bin.right)?;
            let op = lower_bin_op(&bin.op)?;
            Ok(Expression::Binary { op, left: Box::new(left), right: Box::new(right) })
        }
        swc::Expr::Unary(unary) => {
            let arg = lower_expr(&unary.arg)?;
            let op = lower_unary_op(&unary.op)?;
            Ok(Expression::Unary { op, argument: Box::new(arg) })
        }
        swc::Expr::Update(update) => {
            let arg = lower_expr(&update.arg)?;
            let op = if update.op == swc::op!("++") { UpdateOp::Increment } else { UpdateOp::Decrement };
            Ok(Expression::Update { op, argument: Box::new(arg), prefix: update.prefix })
        }
        swc::Expr::Assign(assign) => {
            let left = lower_assign_target(&assign.left)?;
            let right = lower_expr(&assign.right)?;
            if assign.op == swc::AssignOp::Assign {
                Ok(Expression::Assignment { left: Box::new(left), right: Box::new(right) })
            } else {
                let bin_op = assign_op_to_bin(&assign.op)?;
                Ok(Expression::CompoundAssignment {
                    op: bin_op,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
        }
        swc::Expr::Member(member) => {
            let obj = lower_expr(&member.obj)?;
            let (property, computed) = lower_member_prop(&member.prop)?;
            Ok(Expression::Member { object: Box::new(obj), property, computed })
        }
        swc::Expr::SuperProp(_) => Ok(Expression::Undefined),
        swc::Expr::Call(call) => {
            // Callee is an enum, need to match and extract Expr
            let callee = match &call.callee {
                swc::Callee::Expr(expr) => lower_expr(expr)?,
                swc::Callee::Super(_) | swc::Callee::Import(_) => {
                    return Err(LowerError::new("super/import callee not supported"));
                }
            };
            let args: Vec<Expression> = call.args.iter()
                .filter_map(|arg| lower_expr(&arg.expr).ok())
                .collect();
            Ok(Expression::Call { callee: Box::new(callee), arguments: args })
        }
        swc::Expr::New(new_expr) => {
            let constructor = lower_expr(&new_expr.callee)?;
            let args: Vec<Expression> = new_expr.args.as_ref()
                .map(|args| {
                    args.iter().filter_map(|arg| lower_expr(&arg.expr).ok()).collect()
                })
                .unwrap_or_default();
            Ok(Expression::New { constructor: Box::new(constructor), arguments: args })
        }
        swc::Expr::Seq(seq) => {
            let exprs: Vec<Expression> = seq.exprs.iter()
                .filter_map(|e| lower_expr(e).ok())
                .collect();
            Ok(Expression::Sequence(exprs))
        }
        swc::Expr::Cond(cond) => {
            let test = lower_expr(&cond.test)?;
            let consequent = lower_expr(&cond.cons)?;
            let alternate = lower_expr(&cond.alt)?;
            Ok(Expression::Conditional {
                condition: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            })
        }
        swc::Expr::OptChain(opt_chain) => lower_opt_chain(opt_chain),
        swc::Expr::Lit(lit) => lower_literal(lit),
        swc::Expr::TaggedTpl(_) => Err(LowerError::new("Tagged templates not supported")),
        swc::Expr::Tpl(tpl) => lower_template_literal(tpl),
        swc::Expr::Class(_) => Err(LowerError::new("Class expressions not supported")),
        swc::Expr::Invalid(_) => Err(LowerError::new("Invalid expression")),
        swc::Expr::PrivateName(_) => Ok(Expression::Undefined),
        // JSX expressions - not supported
        swc::Expr::JSXMember(_) => Err(LowerError::new("JSX not supported")),
        swc::Expr::JSXNamespacedName(_) => Err(LowerError::new("JSX not supported")),
        swc::Expr::JSXEmpty(_) => Err(LowerError::new("JSX not supported")),
        swc::Expr::JSXElement(_) => Err(LowerError::new("JSX not supported")),
        swc::Expr::JSXFragment(_) => Err(LowerError::new("JSX not supported")),
        // TypeScript expressions - not supported
        swc::Expr::TsTypeAssertion(e) => lower_expr(&e.expr),
        swc::Expr::TsAs(e) => lower_expr(&e.expr),
        swc::Expr::TsSatisfies(e) => lower_expr(&e.expr),
        swc::Expr::TsNonNull(e) => lower_expr(&e.expr),
        swc::Expr::TsConstAssertion(e) => lower_expr(&e.expr),
        swc::Expr::TsInstantiation(e) => lower_expr(&e.expr),
    }
}

fn lower_opt_chain(opt_chain: &swc::OptChainExpr) -> Result<Expression, LowerError> {
    // Handle optional chaining: obj?.prop or obj?.method()
    // SWC's OptChainExpr has:
    // - base: Box<OptChainBase> - the base expression and chain operations
    // - OptChainBase::Member(MemberExpr) - for member access
    // - OptChainBase::Call(OptCall) - for call expression (can contain nested OptChainExpr)
    
    fn process_opt_chain_base(base: &swc::OptChainBase, current_obj: Expression) -> Result<Expression, LowerError> {
        match base {
            swc::OptChainBase::Member(member) => {
                let (property, computed) = lower_member_prop(&member.prop)?;
                // Generate: (obj == null || obj == undefined) ? undefined : obj.prop
                let member_expr = Expression::Member {
                    object: Box::new(current_obj.clone()),
                    property,
                    computed,
                };
                make_optional_check(current_obj, member_expr)
            }
            swc::OptChainBase::Call(opt_call) => {
                // The callee of the call could be another OptChainExpr
                match &*opt_call.callee {
                    swc::Expr::OptChain(nested) => {
                        // obj?.().?() - nested optional chain
                        // First process the inner chain with the current object as base
                        let inner = process_opt_chain_expr(nested, current_obj)?;
                        // Then apply the call args
                        let args: Vec<Expression> = opt_call.args.iter()
                            .filter_map(|arg| lower_expr(&arg.expr).ok())
                            .collect();
                        // For obj?.() - the callee would be obj, so this is just a direct call
                        let call_expr = Expression::Call {
                            callee: Box::new(inner),
                            arguments: args,
                        };
                        // The outer call is already protected by the inner chain
                        Ok(call_expr)
                    }
                    swc::Expr::Member(member) => {
                        // obj?.method(args) - call on member of optional chain
                        let inner_obj = lower_expr(&member.obj)?;
                        let (property, computed) = lower_member_prop(&member.prop)?;
                        // Wrap in optional check
                        let inner_checked = make_optional_check(inner_obj, Expression::Member {
                            object: Box::new(current_obj.clone()),
                            property,
                            computed,
                        })?;
                        let args: Vec<Expression> = opt_call.args.iter()
                            .filter_map(|arg| lower_expr(&arg.expr).ok())
                            .collect();
                        let call_expr = Expression::Call {
                            callee: Box::new(inner_checked),
                            arguments: args,
                        };
                        // The call itself is also optional - if callee is undefined, return undefined
                        make_optional_check(current_obj, call_expr)
                    }
                    swc::Expr::Ident(ident) => {
                        // obj?.func(args) - direct call with optional object
                        let args: Vec<Expression> = opt_call.args.iter()
                            .filter_map(|arg| lower_expr(&arg.expr).ok())
                            .collect();
                        let callee = Expression::Identifier(atom_to_string(&ident.sym));
                        let call_expr = Expression::Call {
                            callee: Box::new(callee),
                            arguments: args,
                        };
                        // Make the call itself optional
                        make_optional_check(current_obj, call_expr)
                    }
                    _ => Err(LowerError::new("Unsupported optional call callee")),
                }
            }
        }
    }
    
    fn process_opt_chain_expr(expr: &swc::OptChainExpr, base_expr: Expression) -> Result<Expression, LowerError> {
        // Process the OptChainBase with the given base expression
        process_opt_chain_base(&expr.base, base_expr)
    }
    
    // For a top-level optional chain, the base is the first expression
    // The OptChainExpr's base contains the first operation
    let base_expr = match &*opt_chain.base {
        swc::OptChainBase::Member(member) => {
            lower_expr(&member.obj)?
        }
        swc::OptChainBase::Call(opt_call) => {
            // For obj?.(), the callee is the object
            match &*opt_call.callee {
                swc::Expr::Member(member) => lower_expr(&member.obj)?,
                swc::Expr::Ident(ident) => Expression::Identifier(atom_to_string(&ident.sym)),
                _ => return Err(LowerError::new("Unsupported optional call base")),
            }
        }
    };
    
    // Now process the chain starting with the base expression
    process_opt_chain_expr(opt_chain, base_expr)
}

/// Helper to create an optional check: (obj == null || obj == undefined) ? undefined : expr
fn make_optional_check(obj: Expression, expr: Expression) -> Result<Expression, LowerError> {
    let null_check = Expression::Binary {
        op: BinaryOp::Or,
        left: Box::new(Expression::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(obj.clone()),
            right: Box::new(Expression::Null),
        }),
        right: Box::new(Expression::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(obj),
            right: Box::new(Expression::Undefined),
        }),
    };
    Ok(Expression::Conditional {
        condition: Box::new(null_check),
        consequent: Box::new(Expression::Undefined),
        alternate: Box::new(expr),
    })
}

fn lower_member_prop(prop: &swc::MemberProp) -> Result<(PropertyKey, bool), LowerError> {
    match prop {
        swc::MemberProp::Ident(ident) => Ok((PropertyKey::Ident(atom_to_string(&ident.sym)), false)),
        swc::MemberProp::PrivateName(_) => Err(LowerError::new("Private names not supported")),
        swc::MemberProp::Computed(expr) => {
            // Lower the computed expression
            let expr = lower_expr(&expr.expr)?;
            Ok((PropertyKey::Computed(Box::new(expr)), true))
        }
    }
}

fn lower_assign_target(target: &swc::AssignTarget) -> Result<Expression, LowerError> {
    match target {
        swc::AssignTarget::Simple(simple) => lower_simple_assign_target(simple),
        swc::AssignTarget::Pat(_) => Err(LowerError::new("Destructuring assignment not supported")),
    }
}

fn lower_simple_assign_target(target: &swc::SimpleAssignTarget) -> Result<Expression, LowerError> {
    match target {
        swc::SimpleAssignTarget::Ident(ident) => Ok(Expression::Identifier(atom_to_string(&ident.id.sym))),
        swc::SimpleAssignTarget::Member(member) => {
            let obj = lower_expr(&member.obj)?;
            let (property, computed) = lower_member_prop(&member.prop)?;
            Ok(Expression::Member { object: Box::new(obj), property, computed })
        }
        _ => Err(LowerError::new("Complex assignment target not supported")),
    }
}

fn lower_prop_or_spread(prop: &swc::PropOrSpread) -> Result<(PropertyKey, PropertyValue), LowerError> {
    match prop {
        swc::PropOrSpread::Prop(prop) => lower_prop(prop),
        swc::PropOrSpread::Spread(_) => Err(LowerError::new("Spread not supported")),
    }
}

fn lower_literal(lit: &swc::Lit) -> Result<Expression, LowerError> {
    match lit {
        swc::Lit::Num(n) => Ok(Expression::Number(n.value)),
        swc::Lit::Str(s) => Ok(Expression::String(wtf8_atom_to_string(&s.value))),
        swc::Lit::Bool(b) => Ok(Expression::Boolean(b.value)),
        swc::Lit::Null(_) => Ok(Expression::Null),
        swc::Lit::Regex(regex) => Ok(Expression::String(format!("/{}/{}", regex.exp, regex.flags))),
        swc::Lit::BigInt(_) => Err(LowerError::new("BigInt not supported")),
        swc::Lit::JSXText(t) => Ok(Expression::String(t.value.to_string())),
    }
}

fn lower_template_literal(tpl: &swc::Tpl) -> Result<Expression, LowerError> {
    // If there are no expressions, just return the string parts joined together
    if tpl.exprs.is_empty() {
        let mut result = String::new();
        for elem in &tpl.quasis {
            if let Some(cooked) = &elem.cooked {
                result.push_str(&wtf8_atom_to_string(cooked));
            }
        }
        return Ok(Expression::String(result));
    }
    
    // Build a sequence expression that concatenates all parts
    let mut exprs: Vec<Expression> = Vec::new();
    let quasi_count = tpl.quasis.len();
    let expr_count = tpl.exprs.len();
    
    for i in 0..quasi_count {
        // Add the quasi part (string before this position)
        if let Some(cooked) = &tpl.quasis.get(i).and_then(|q| q.cooked.as_ref()) {
            let s = wtf8_atom_to_string(cooked);
            if !s.is_empty() {
                exprs.push(Expression::String(s));
            }
        }
        
        // Add the corresponding expression (if any)
        if i < expr_count {
            exprs.push(lower_expr(&tpl.exprs[i])?);
        }
    }
    
    // If we have a single string, return it directly
    if exprs.len() == 1 {
        return Ok(exprs.remove(0));
    }
    
    // Build nested binary adds: ((s1 + e1) + s2) + e2 ...
    let mut result = exprs.remove(0);
    while !exprs.is_empty() {
        let right = exprs.remove(0);
        result = Expression::Binary {
            op: BinaryOp::Add,
            left: Box::new(result),
            right: Box::new(right),
        };
    }
    
    Ok(result)
}

fn lower_prop(prop: &swc::Prop) -> Result<(PropertyKey, PropertyValue), LowerError> {
    match prop {
        swc::Prop::Shorthand(ident) => {
            // Shorthand has just the ident
            let name = atom_to_string(&ident.sym);
            Ok((PropertyKey::Ident(name.clone()), PropertyValue::Value(Expression::Identifier(name))))
        }
        swc::Prop::KeyValue(kv) => {
            let key = lower_prop_name(&kv.key)?;
            let value = lower_expr(&kv.value)?;
            Ok((key, PropertyValue::Value(value)))
        }
        swc::Prop::Getter(getter) => {
            let key = lower_prop_name(&getter.key)?;
            let body = getter.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Ok((key, PropertyValue::Getter { params: vec![], body }))
        }
        swc::Prop::Setter(setter) => {
            let key = lower_prop_name(&setter.key)?;
            let param = match &*setter.param {
                swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                _ => "value".to_string(),
            };
            let body = setter.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Ok((key, PropertyValue::Setter { param, body }))
        }
        swc::Prop::Method(method) => {
            let key = lower_prop_name(&method.key)?;
            let params = method.function.params.iter().map(|p| {
                match &p.pat {
                    swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                    _ => "arg".to_string(),
                }
            }).collect();
            let body = method.function.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Ok((key, PropertyValue::Value(Expression::FunctionExpression { name: None, params, body })))
        }
        swc::Prop::Assign(_) => Err(LowerError::new("Assignment property not supported")),
    }
}

fn lower_prop_name(key: &swc::PropName) -> Result<PropertyKey, LowerError> {
    match key {
        swc::PropName::Str(s) => Ok(PropertyKey::String(wtf8_atom_to_string(&s.value))),
        swc::PropName::Ident(i) => Ok(PropertyKey::Ident(atom_to_string(&i.sym))),
        swc::PropName::Num(n) => Ok(PropertyKey::Number(n.value)),
        swc::PropName::Computed(_) => Err(LowerError::new("Computed property name not supported")),
        swc::PropName::BigInt(b) => Ok(PropertyKey::String(b.value.to_string())),
    }
}

fn lower_bin_op(op: &swc::BinaryOp) -> Result<BinaryOp, LowerError> {
    match op {
        swc::BinaryOp::Mul => Ok(BinaryOp::Mul),
        swc::BinaryOp::Div => Ok(BinaryOp::Div),
        swc::BinaryOp::Mod => Ok(BinaryOp::Mod),
        swc::BinaryOp::Add => Ok(BinaryOp::Add),
        swc::BinaryOp::Sub => Ok(BinaryOp::Sub),
        swc::BinaryOp::LShift => Ok(BinaryOp::Shl),
        swc::BinaryOp::RShift => Ok(BinaryOp::Shr),
        swc::BinaryOp::ZeroFillRShift => Ok(BinaryOp::Ushr),
        swc::BinaryOp::Lt => Ok(BinaryOp::Lt),
        swc::BinaryOp::LtEq => Ok(BinaryOp::Le),
        swc::BinaryOp::Gt => Ok(BinaryOp::Gt),
        swc::BinaryOp::GtEq => Ok(BinaryOp::Ge),
        swc::BinaryOp::EqEq => Ok(BinaryOp::Eq),
        swc::BinaryOp::EqEqEq => Ok(BinaryOp::StrictEq),
        swc::BinaryOp::NotEq => Ok(BinaryOp::Neq),
        swc::BinaryOp::NotEqEq => Ok(BinaryOp::StrictNeq),
        swc::BinaryOp::BitAnd => Ok(BinaryOp::BitAnd),
        swc::BinaryOp::BitXor => Ok(BinaryOp::BitXor),
        swc::BinaryOp::BitOr => Ok(BinaryOp::BitOr),
        swc::BinaryOp::LogicalAnd => Ok(BinaryOp::And),
        swc::BinaryOp::LogicalOr => Ok(BinaryOp::Or),
        swc::BinaryOp::NullishCoalescing => Ok(BinaryOp::NullishCoalescing),
        swc::BinaryOp::In => Ok(BinaryOp::In),
        swc::BinaryOp::InstanceOf => Ok(BinaryOp::Instanceof),
        _ => Err(LowerError::new(format!("Unsupported binary operator: {:?}", op))),
    }
}

fn lower_unary_op(op: &swc::UnaryOp) -> Result<UnaryOp, LowerError> {
    match op {
        swc::UnaryOp::Minus => Ok(UnaryOp::Neg),
        swc::UnaryOp::Plus => Err(LowerError::new("Unary + not supported")),
        swc::UnaryOp::Tilde => Ok(UnaryOp::BitNot),
        swc::UnaryOp::Bang => Ok(UnaryOp::Not),
        swc::UnaryOp::TypeOf => Ok(UnaryOp::Typeof),
        swc::UnaryOp::Void => Ok(UnaryOp::Void),
        swc::UnaryOp::Delete => Err(LowerError::new("Delete not supported")),
    }
}

fn assign_op_to_bin(op: &swc::AssignOp) -> Result<CompoundOp, LowerError> {
    match op {
        swc::AssignOp::AddAssign => Ok(CompoundOp::Add),
        swc::AssignOp::SubAssign => Ok(CompoundOp::Sub),
        swc::AssignOp::MulAssign => Ok(CompoundOp::Mul),
        swc::AssignOp::DivAssign => Ok(CompoundOp::Div),
        swc::AssignOp::ModAssign => Ok(CompoundOp::Mod),
        swc::AssignOp::LShiftAssign => Ok(CompoundOp::Shl),
        swc::AssignOp::RShiftAssign => Ok(CompoundOp::Shr),
        swc::AssignOp::ZeroFillRShiftAssign => Ok(CompoundOp::Ushr),
        swc::AssignOp::BitAndAssign => Ok(CompoundOp::BitAnd),
        swc::AssignOp::BitXorAssign => Ok(CompoundOp::BitXor),
        swc::AssignOp::BitOrAssign => Ok(CompoundOp::BitOr),
        _ => Err(LowerError::new(format!("Unsupported assign operator: {:?}", op))),
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use crate::hir::HirFunction;

    #[test]
    fn test_lower_simple() {
        // Just verify it compiles - test that lower_hir module is accessible
        fn _check_hir_function(_: Option<HirFunction>) {}
        _check_hir_function(None);
    }
}
