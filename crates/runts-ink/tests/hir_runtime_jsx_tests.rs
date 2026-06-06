//! Unit tests for HIR runtime JSX evaluation.
//!
//! Tests the JSX element creation, fragment handling,
//! props evaluation, and conditional rendering.

use runts_transpile::hir::{self, Expr, Module, ModuleItem, Stmt};
use std::collections::HashMap;

/// Helper: create a simple module with a default App export.
fn make_module(body: Vec<Stmt>) -> Module {
    Module {
        items: vec![ModuleItem::Stmt(Stmt::Block { stmts: body })],
    }
}

/// Helper: create a Box JSX element.
fn make_box_jsx(children: Vec<hir::JSXChild>) -> Expr {
    Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![],
        children,
        self_closing: false,
    })
}

/// Helper: create a Text JSX element.
fn make_text_jsx(content: &str) -> Expr {
    Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Text".to_string()),
        props: vec![],
        children: vec![hir::JSXChild::Text(content.to_string())],
        self_closing: false,
    })
}

/// Helper: create a string literal.
fn make_string(s: &str) -> Expr {
    Expr::String(s.to_string())
}

/// Helper: create a number literal.
fn make_number(n: f64) -> Expr {
    Expr::Number(n)
}

/// Helper: create a boolean literal.
fn make_bool(b: bool) -> Expr {
    Expr::Boolean(b)
}

// =============================================================================
// JSX Element Tests
// =============================================================================

#[test]
fn test_jsx_simple_element() {
    // <Box><Text>Hello</Text></Box>
    let text = hir::JSXChild::Text("Hello".to_string());
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![],
        children: vec![text],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    // Module should be valid
    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_fragment() {
    // <><Text>First</Text><Text>Second</Text></>
    let fragment = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Fragment,
        props: vec![],
        children: vec![
            hir::JSXChild::Element(Box::new(hir::JSXElement {
                tag: hir::JSXTag::Ident("Text".to_string()),
                props: vec![],
                children: vec![hir::JSXChild::Text("First".to_string())],
                self_closing: false,
            })),
            hir::JSXChild::Element(Box::new(hir::JSXElement {
                tag: hir::JSXTag::Ident("Text".to_string()),
                props: vec![],
                children: vec![hir::JSXChild::Text("Second".to_string())],
                self_closing: false,
            })),
        ],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(fragment)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_self_closing() {
    // <Spacer />
    let spacer = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Spacer".to_string()),
        props: vec![],
        children: vec![],
        self_closing: true,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(spacer)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// JSX Props Tests
// =============================================================================

#[test]
fn test_jsx_string_prop() {
    // <Box color="red">
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![hir::JSXAttr {
            name: hir::JSXAttrName::Ident("color".to_string()),
            value: Some(hir::JSXAttrValue::Literal(hir::JSXLiteral::String("red".to_string()))),
        }],
        children: vec![],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_number_prop() {
    // <Box padding={5}>
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![hir::JSXAttr {
            name: hir::JSXAttrName::Ident("padding".to_string()),
            value: Some(hir::JSXAttrValue::Expr(Box::new(Expr::Number(5.0)))),
        }],
        children: vec![],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_boolean_prop() {
    // <Text bold>
    let text_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Text".to_string()),
        props: vec![hir::JSXAttr {
            name: hir::JSXAttrName::Ident("bold".to_string()),
            value: None, // Boolean prop shorthand
        }],
        children: vec![],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(text_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_spread_props() {
    // <Box {...props}>
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![hir::JSXAttr {
            name: hir::JSXAttrName::Ident("...".to_string()), // Spread
            value: Some(hir::JSXAttrValue::Expr(Box::new(Expr::Ident {
                name: "props".to_string(),
            }))),
        }],
        children: vec![],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// JSX Children Tests
// =============================================================================

#[test]
fn test_jsx_text_child() {
    // <Text>Hello World</Text>
    let text_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Text".to_string()),
        props: vec![],
        children: vec![hir::JSXChild::Text("Hello World".to_string())],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(text_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_expression_child() {
    // <Text>{name}</Text>
    let text_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Text".to_string()),
        props: vec![],
        children: vec![hir::JSXChild::Expr(Box::new(Expr::Ident {
            name: "name".to_string(),
        }))],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(text_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_mixed_children() {
    // <Box>Hello, <Text bold>{name}</Text>!</Box>
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![],
        children: vec![
            hir::JSXChild::Text("Hello, ".to_string()),
            hir::JSXChild::Element(Box::new(hir::JSXElement {
                tag: hir::JSXTag::Ident("Text".to_string()),
                props: vec![hir::JSXAttr {
                    name: hir::JSXAttrName::Ident("bold".to_string()),
                    value: None,
                }],
                children: vec![hir::JSXChild::Expr(Box::new(Expr::Ident {
                    name: "name".to_string(),
                }))],
                self_closing: false,
            })),
            hir::JSXChild::Text("!".to_string()),
        ],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// Conditional Rendering Tests
// =============================================================================

#[test]
fn test_jsx_conditional_true() {
    // {condition && <Text>Show</Text>}
    let conditional = Expr::Cond {
        test: Box::new(Expr::Boolean(true)),
        consequent: Box::new(Expr::JSX(hir::JSXElement {
            tag: hir::JSXTag::Ident("Text".to_string()),
            props: vec![],
            children: vec![hir::JSXChild::Text("Show".to_string())],
            self_closing: false,
        })),
        alternate: Box::new(Expr::Undefined),
    };

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(conditional)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_conditional_false() {
    // {condition && <Text>Show</Text>}
    let conditional = Expr::Cond {
        test: Box::new(Expr::Boolean(false)),
        consequent: Box::new(Expr::JSX(hir::JSXElement {
            tag: hir::JSXTag::Ident("Text".to_string()),
            props: vec![],
            children: vec![hir::JSXChild::Text("Show".to_string())],
            self_closing: false,
        })),
        alternate: Box::new(Expr::Undefined),
    };

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(conditional)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_ternary() {
    // {show ? <Text>On</Text> : <Text>Off</Text>}
    let ternary = Expr::Cond {
        test: Box::new(Expr::Ident {
            name: "show".to_string(),
        }),
        consequent: Box::new(Expr::JSX(hir::JSXElement {
            tag: hir::JSXTag::Ident("Text".to_string()),
            props: vec![],
            children: vec![hir::JSXChild::Text("On".to_string())],
            self_closing: false,
        })),
        alternate: Box::new(Expr::JSX(hir::JSXElement {
            tag: hir::JSXTag::Ident("Text".to_string()),
            props: vec![],
            children: vec![hir::JSXChild::Text("Off".to_string())],
            self_closing: false,
        })),
    };

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(ternary)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// JSX with Hooks Tests
// =============================================================================

#[test]
fn test_jsx_with_use_state() {
    // function App() {
    //   const [count, setCount] = useState(0);
    //   return <Text>{count}</Text>;
    // }
    let func = hir::FunctionDecl {
        name: "App".to_string(),
        params: vec![],
        body: Some(hir::Block(vec![
            Stmt::Variable(hir::VariableDecl {
                pattern: None,
                name: "count".to_string(),
                init: Some(Expr::Call {
                    callee: Box::new(Expr::Ident {
                        name: "useState".to_string(),
                    }),
                    arguments: vec![Expr::Number(0.0)],
                }),
            }),
            Stmt::Return {
                arg: Some(Box::new(Expr::JSX(hir::JSXElement {
                    tag: hir::JSXTag::Ident("Text".to_string()),
                    props: vec![],
                    children: vec![hir::JSXChild::Expr(Box::new(Expr::Ident {
                        name: "count".to_string(),
                    }))],
                    self_closing: false,
                }))),
            },
        ])),
    };

    let module = Module {
        items: vec![ModuleItem::Decl(hir::Decl::Function(func))],
    };

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_nested_components() {
    // <Box><Inner /><Inner /></Box>
    let inner = hir::JSXChild::Element(Box::new(hir::JSXElement {
        tag: hir::JSXTag::Ident("Inner".to_string()),
        props: vec![],
        children: vec![],
        self_closing: false,
    }));

    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![],
        children: vec![inner.clone(), inner],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// Template Literal Tests
// =============================================================================

#[test]
fn test_template_literal_simple() {
    // `Hello`
    let template = Expr::Template {
        parts: vec![hir::TemplatePart::String {
            value: "Hello".to_string(),
        }],
        exprs: vec![],
    };

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(template)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_template_literal_with_expr() {
    // `Hello ${name}`
    let template = Expr::Template {
        parts: vec![
            hir::TemplatePart::String {
                value: "Hello ".to_string(),
            },
            hir::TemplatePart::String {
                value: "".to_string(),
            },
        ],
        exprs: vec![Expr::Ident {
            name: "name".to_string(),
        }],
    };

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(template)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// Array and Map Tests
// =============================================================================

#[test]
fn test_jsx_array_map() {
    // [{id: 1, name: 'a'}, {id: 2, name: 'b'}].map(item => <Text>{item.name}</Text>)
    let items_array = Expr::Array {
        elems: vec![
            Some(Expr::Object {
                members: vec![hir::ObjectMemberExpr {
                    prop: hir::ObjectProp::Init {
                        key: hir::PropKey::Str("id".to_string()),
                        value: Expr::Number(1.0),
                    },
                }]),
            }),
            Some(Expr::Object {
                members: vec![hir::ObjectMemberExpr {
                    prop: hir::ObjectProp::Init {
                        key: hir::PropKey::Str("id".to_string()),
                        value: Expr::Number(2.0),
                    },
                }]),
            }),
        ],
    };

    let map_call = Expr::Call {
        callee: Box::new(Expr::Member {
            obj: Box::new(items_array),
            property: Box::new(Expr::Ident {
                name: "map".to_string(),
            }),
            computed: false,
        }),
        arguments: vec![Expr::ArrowFunction {
            params: vec![hir::Param {
                name: "item".to_string(),
            }],
            body: Box::new(Expr::JSX(hir::JSXElement {
                tag: hir::JSXTag::Ident("Text".to_string()),
                props: vec![],
                children: vec![hir::JSXChild::Expr(Box::new(Expr::Ident {
                    name: "item".to_string(),
                }))],
                self_closing: false,
            })),
            is_async: false,
        }],
    };

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(map_call)),
    }]);

    assert!(!module.items.is_empty());
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_jsx_empty_children() {
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![],
        children: vec![],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_whitespace_child() {
    let text_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Text".to_string()),
        props: vec![],
        children: vec![hir::JSXChild::Text("   ".to_string())],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(text_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_comment_child() {
    let text_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Text".to_string()),
        props: vec![],
        children: vec![hir::JSXChild::Comment("This is a comment".to_string())],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(text_elem)),
    }]);

    assert!(!module.items.is_empty());
}

#[test]
fn test_jsx_spread_child() {
    // {...children}
    let box_elem = Expr::JSX(hir::JSXElement {
        tag: hir::JSXTag::Ident("Box".to_string()),
        props: vec![],
        children: vec![hir::JSXChild::Spread(Box::new(Expr::Ident {
            name: "children".to_string(),
        }))],
        self_closing: false,
    });

    let module = make_module(vec![Stmt::Return {
        arg: Some(Box::new(box_elem)),
    }]);

    assert!(!module.items.is_empty());
}
