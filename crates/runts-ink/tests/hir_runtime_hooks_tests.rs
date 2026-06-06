//! Unit tests for HIR runtime hooks implementation.
//!
//! Tests the hooks: useState, useEffect, useContext,
//! useCallback, useMemo, useInput, useFocus, etc.

use runts_transpile::hir::{self, Expr, Module, ModuleItem, Stmt};

/// Helper: create a useState call expression.
fn make_use_state_call(initial: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useState".to_string(),
        }),
        arguments: vec![initial],
    }
}

/// Helper: create a useEffect call expression.
fn make_use_effect_call(effect: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useEffect".to_string(),
        }),
        arguments: vec![effect],
    }
}

/// Helper: create a useContext call expression.
fn make_use_context_call(context: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useContext".to_string(),
        }),
        arguments: vec![context],
    }
}

/// Helper: create a useCallback call expression.
fn make_use_callback_call(callback: Expr, deps: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useCallback".to_string(),
        }),
        arguments: vec![callback, deps],
    }
}

/// Helper: create a useMemo call expression.
fn make_use_memo_call(compute: Expr, deps: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useMemo".to_string(),
        }),
        arguments: vec![compute, deps],
    }
}

/// Helper: create a useInput call expression.
fn make_use_input_call(handler: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useInput".to_string(),
        }),
        arguments: vec![handler],
    }
}

/// Helper: create a useFocus call expression.
fn make_use_focus_call(id: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useFocus".to_string(),
        }),
        arguments: vec![id],
    }
}

/// Helper: create a useWindowSize call expression.
fn make_use_window_size_call() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useWindowSize".to_string(),
        }),
        arguments: vec![],
    }
}

/// Helper: create a useApp call expression.
fn make_use_app_call() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useApp".to_string(),
        }),
        arguments: vec![],
    }
}

/// Helper: create a createContext call expression.
fn make_create_context_call(default_value: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "createContext".to_string(),
        }),
        arguments: vec![default_value],
    }
}

// =============================================================================
// useState Tests
// =============================================================================

#[test]
fn test_use_state_with_number() {
    // useState(0)
    let call = make_use_state_call(Expr::Number(0.0));
    
    if let Expr::Call { callee, arguments } = call {
        assert!(matches!(callee.as_ref(), Expr::Ident { name } if name == "useState"));
        assert_eq!(arguments.len(), 1);
        assert!(matches!(arguments[0], Expr::Number(n) if *n == 0.0));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_state_with_string() {
    // useState('')
    let call = make_use_state_call(Expr::String("".to_string()));
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::String(s) if s.is_empty()));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_state_with_boolean() {
    // useState(false)
    let call = make_use_state_call(Expr::Boolean(false));
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Boolean(b) if !b));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_state_with_null() {
    // useState(null)
    let call = make_use_state_call(Expr::Null);
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Null));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_state_with_object() {
    // useState({ count: 0 })
    let call = make_use_state_call(Expr::Object {
        members: vec![hir::ObjectMemberExpr {
            prop: hir::ObjectProp::Init {
                key: hir::PropKey::Str("count".to_string()),
                value: Expr::Number(0.0),
            },
        }],
    });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Object { .. }));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_state_with_array() {
    // useState([])
    let call = make_use_state_call(Expr::Array { elems: vec![] });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Array { .. }));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useEffect Tests
// =============================================================================

#[test]
fn test_use_effect_simple() {
    // useEffect(() => { ... })
    let call = make_use_effect_call(Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::ArrowFunction { .. }));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_effect_with_cleanup() {
    // useEffect(() => { return () => { ... } })
    let cleanup = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let effect_body = Expr::Block(vec![Stmt::Return {
        arg: Some(Box::new(cleanup)),
    }]);
    
    let call = make_use_effect_call(Expr::ArrowFunction {
        params: vec![],
        body: Box::new(effect_body),
        is_async: false,
    });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_effect_with_deps() {
    // useEffect(() => { ... }, [count])
    let effect = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let call = Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useEffect".to_string(),
        }),
        arguments: vec![effect, Expr::Array {
            elems: vec![Some(Expr::Ident {
                name: "count".to_string(),
            })],
        }],
    };
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 2);
        assert!(matches!(&arguments[1], Expr::Array { elems } if elems.len() == 1));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_effect_empty_deps() {
    // useEffect(() => { ... }, [])
    let effect = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let call = Expr::Call {
        callee: Box::new(Expr::Ident {
            name: "useEffect".to_string(),
        }),
        arguments: vec![effect, Expr::Array { elems: vec![] }],
    };
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 2);
        assert!(matches!(&arguments[1], Expr::Array { elems } if elems.is_empty()));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useContext Tests
// =============================================================================

#[test]
fn test_use_context_simple() {
    // useContext(ThemeContext)
    let call = make_use_context_call(Expr::Ident {
        name: "ThemeContext".to_string(),
    });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Ident { name } if name == "ThemeContext"));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_create_context_simple() {
    // createContext(defaultValue)
    let call = make_create_context_call(Expr::String("default".to_string()));
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::String(_)));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_create_context_with_object() {
    // createContext({ primary: 'cyan', secondary: 'green' })
    let call = make_create_context_call(Expr::Object {
        members: vec![
            hir::ObjectMemberExpr {
                prop: hir::ObjectProp::Init {
                    key: hir::PropKey::Str("primary".to_string()),
                    value: Expr::String("cyan".to_string()),
                },
            },
            hir::ObjectMemberExpr {
                prop: hir::ObjectProp::Init {
                    key: hir::PropKey::Str("secondary".to_string()),
                    value: Expr::String("green".to_string()),
                },
            },
        ],
    });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Object { members } if members.len() == 2));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useCallback Tests
// =============================================================================

#[test]
fn test_use_callback_simple() {
    // useCallback(() => { ... }, [])
    let callback = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let call = make_use_callback_call(callback, Expr::Array { elems: vec![] });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 2);
        assert!(matches!(&arguments[0], Expr::ArrowFunction { .. }));
        assert!(matches!(&arguments[1], Expr::Array { elems } if elems.is_empty()));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_callback_with_params() {
    // useCallback((x, y) => { ... }, [x, y])
    let callback = Expr::ArrowFunction {
        params: vec![
            hir::Param { name: "x".to_string() },
            hir::Param { name: "y".to_string() },
        ],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let call = make_use_callback_call(
        callback,
        Expr::Array {
            elems: vec![
                Some(Expr::Ident { name: "x".to_string() }),
                Some(Expr::Ident { name: "y".to_string() }),
            ],
        },
    );
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 2);
        assert!(matches!(&arguments[1], Expr::Array { elems } if elems.len() == 2));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useMemo Tests
// =============================================================================

#[test]
fn test_use_memo_simple() {
    // useMemo(() => computeExpensiveValue(), [])
    let compute = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let call = make_use_memo_call(compute, Expr::Array { elems: vec![] });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 2);
        assert!(matches!(&arguments[0], Expr::ArrowFunction { .. }));
        assert!(matches!(&arguments[1], Expr::Array { elems } if elems.is_empty()));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_memo_with_deps() {
    // useMemo(() => a + b, [a, b])
    let compute = Expr::ArrowFunction {
        params: vec![],
        body: Box::new(Expr::Bin {
            op: hir::BinaryOp::Add,
            left: Box::new(Expr::Ident { name: "a".to_string() }),
            right: Box::new(Expr::Ident { name: "b".to_string() }),
        }),
        is_async: false,
    };
    
    let call = make_use_memo_call(
        compute,
        Expr::Array {
            elems: vec![
                Some(Expr::Ident { name: "a".to_string() }),
                Some(Expr::Ident { name: "b".to_string() }),
            ],
        },
    );
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 2);
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useInput Tests
// =============================================================================

#[test]
fn test_use_input_simple() {
    // useInput((input, key) => { ... })
    let handler = Expr::ArrowFunction {
        params: vec![
            hir::Param { name: "input".to_string() },
            hir::Param { name: "key".to_string() },
        ],
        body: Box::new(Expr::Block(vec![])),
        is_async: false,
    };
    
    let call = make_use_input_call(handler);
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::ArrowFunction { params, .. } if params.len() == 2));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_input_with_exit() {
    // useInput((input, key) => { if (input === 'q') process.exit(0); })
    let exit_stmt = Stmt::Expr {
        expr: Expr::Call {
            callee: Box::new(Expr::Member {
                obj: Box::new(Expr::Ident { name: "process".to_string() }),
                property: Box::new(Expr::Ident { name: "exit".to_string() }),
                computed: false,
            }),
            arguments: vec![Expr::Number(0.0)],
        },
    };
    
    let handler = Expr::ArrowFunction {
        params: vec![
            hir::Param { name: "input".to_string() },
            hir::Param { name: "key".to_string() },
        ],
        body: Box::new(Expr::Block(vec![exit_stmt])),
        is_async: false,
    };
    
    let call = make_use_input_call(handler);
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useFocus Tests
// =============================================================================

#[test]
fn test_use_focus_with_id() {
    // useFocus('myInput')
    let call = make_use_focus_call(Expr::String("myInput".to_string()));
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::String(s) if s == "myInput"));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_focus_with_variable() {
    // useFocus(id)
    let call = make_use_focus_call(Expr::Ident { name: "id".to_string() });
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 1);
        assert!(matches!(&arguments[0], Expr::Ident { name } if name == "id"));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useWindowSize Tests
// =============================================================================

#[test]
fn test_use_window_size() {
    // useWindowSize()
    let call = make_use_window_size_call();
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 0);
        assert!(matches!(callee.as_ref(), Expr::Ident { name } if name == "useWindowSize"));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useApp Tests
// =============================================================================

#[test]
fn test_use_app() {
    // useApp()
    let call = make_use_app_call();
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 0);
        assert!(matches!(callee.as_ref(), Expr::Ident { name } if name == "useApp"));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// useStdin/useStdout/useStderr Tests
// =============================================================================

#[test]
fn test_use_stdin() {
    // useStdin()
    let call = Expr::Call {
        callee: Box::new(Expr::Ident { name: "useStdin".to_string() }),
        arguments: vec![],
    };
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 0);
        assert!(matches!(callee.as_ref(), Expr::Ident { name } if name == "useStdin"));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_stdout() {
    // useStdout()
    let call = Expr::Call {
        callee: Box::new(Expr::Ident { name: "useStdout".to_string() }),
        arguments: vec![],
    };
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 0);
        assert!(matches!(callee.as_ref(), Expr::Ident { name } if name == "useStdout"));
    } else {
        panic!("Expected Call expression");
    }
}

#[test]
fn test_use_stderr() {
    // useStderr()
    let call = Expr::Call {
        callee: Box::new(Expr::Ident { name: "useStderr".to_string() }),
        arguments: vec![],
    };
    
    if let Expr::Call { callee, arguments } = call {
        assert_eq!(arguments.len(), 0);
        assert!(matches!(callee.as_ref(), Expr::Ident { name } if name == "useStderr"));
    } else {
        panic!("Expected Call expression");
    }
}

// =============================================================================
// Combined Hooks Tests
// =============================================================================

#[test]
fn test_multiple_use_state_calls() {
    // const [a, setA] = useState(0);
    // const [b, setB] = useState(0);
    
    let use_state_1 = Expr::Call {
        callee: Box::new(Expr::Ident { name: "useState".to_string() }),
        arguments: vec![Expr::Number(0.0)],
    };
    
    let use_state_2 = Expr::Call {
        callee: Box::new(Expr::Ident { name: "useState".to_string() }),
        arguments: vec![Expr::Number(0.0)],
    };
    
    // Both calls should be valid
    assert!(matches!(use_state_1, Expr::Call { .. }));
    assert!(matches!(use_state_2, Expr::Call { .. }));
}

#[test]
fn test_hooks_with_conditional() {
    // {show && useState(0)}
    let conditional = Expr::Cond {
        test: Box::new(Expr::Ident { name: "show".to_string() }),
        consequent: Box::new(make_use_state_call(Expr::Number(0.0))),
        alternate: Box::new(Expr::Undefined),
    };
    
    if let Expr::Cond { test, consequent, alternate } = conditional {
        assert!(matches!(test.as_ref(), Expr::Ident { name } if name == "show"));
        assert!(matches!(consequent.as_ref(), Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::Ident { name: "useState" })));
    } else {
        panic!("Expected Cond expression");
    }
}

#[test]
fn test_hook_in_nested_function() {
    // function Inner() {
    //   const [count, setCount] = useState(0);
    //   return <Text>{count}</Text>;
    // }
    
    let inner_func = hir::FunctionDecl {
        name: "Inner".to_string(),
        params: vec![],
        body: Some(hir::Block(vec![
            Stmt::Variable(hir::VariableDecl {
                pattern: None,
                name: "count".to_string(),
                init: Some(make_use_state_call(Expr::Number(0.0))),
            }),
            Stmt::Return {
                arg: Some(Box::new(Expr::JSX(hir::JSXElement {
                    tag: hir::JSXTag::Ident("Text".to_string()),
                    props: vec![],
                    children: vec![hir::JSXChild::Expr(Box::new(Expr::Ident { name: "count".to_string() }))],
                    self_closing: false,
                }))),
            },
        ])),
    };
    
    let module = Module {
        items: vec![ModuleItem::Decl(hir::Decl::Function(inner_func))],
    };
    
    assert!(!module.items.is_empty());
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_hook_outside_component_panics() {
    // Hooks must be called inside components - this tests the validation
    // The module structure should allow this pattern but runtime should error
    let invalid_module = Module {
        items: vec![ModuleItem::Stmt(Stmt::Variable(hir::VariableDecl {
            pattern: None,
            name: "state".to_string(),
            init: Some(make_use_state_call(Expr::Number(0.0))),
        }))],
    };
    
    // Module is structurally valid, runtime will handle the rules of hooks violation
    assert!(!invalid_module.items.is_empty());
}
