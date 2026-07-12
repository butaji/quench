//! JSX expression evaluation
//!
//! This module handles evaluation of JSX elements and fragments,
//! converting them into calls to the virtual DOM library (ink).

use crate::ast::{Expression, JsxAttrValue, JsxChild, JsxProp, JsxTagName};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::value::{JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a JSX element expression
pub fn eval_jsx_element(
    tag: &crate::ast::JsxTagName,
    props: &[crate::ast::JsxProp],
    children: &[crate::ast::JsxChild],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    use crate::ast::PropertyKey;

    // Build the createElement call
    let create_element_fn = Expression::Member {
        object: Box::new(Expression::Identifier("ink".to_string())),
        property: PropertyKey::Ident("createElement".to_string()),
        computed: false,
    };

    // Build props object
    let prop_entries = build_jsx_props(props)?;

    // Build props object expression
    let props_expr = Expression::Object(prop_entries);

    // Arguments: [tag, props, ...children]
    let mut args = vec![Expression::String(tag_name_to_string(tag))];
    args.push(props_expr);
    args.extend(build_jsx_children_args(children));

    // Call createElement
    let call_expr = Expression::Call {
        callee: Box::new(create_element_fn),
        arguments: args,
    };

    eval_expression(&call_expr, env, false)
}

/// Evaluate a JSX fragment expression
pub fn eval_jsx_fragment(
    children: &[crate::ast::JsxChild],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    use crate::ast::PropertyKey;

    // Fragment is lowered to: createElement(Fragment, null, ...children)
    let create_element_fn = Expression::Member {
        object: Box::new(Expression::Identifier("ink".to_string())),
        property: PropertyKey::Ident("createElement".to_string()),
        computed: false,
    };

    let args = vec![
        Expression::Member {
            object: Box::new(Expression::Identifier("ink".to_string())),
            property: PropertyKey::Ident("Fragment".to_string()),
            computed: false,
        },
        Expression::Null,
    ];

    let mut call_args = args;
    call_args.extend(build_jsx_children_args(children));

    let call_expr = Expression::Call {
        callee: Box::new(create_element_fn),
        arguments: call_args,
    };

    eval_expression(&call_expr, env, false)
}

/// Build props object entries from JSX props
fn build_jsx_props(
    props: &[JsxProp],
) -> Result<Vec<(crate::ast::PropertyKey, crate::ast::PropertyValue)>, JsError> {
    let mut prop_entries = Vec::new();

    for prop in props {
        match prop {
            JsxProp::Attr { name, value } => {
                let prop_value = match value {
                    JsxAttrValue::String(s) => {
                        crate::ast::PropertyValue::Value(Expression::String(s.clone()))
                    }
                    JsxAttrValue::Expression(expr) => {
                        crate::ast::PropertyValue::Value(expr.clone())
                    }
                };
                prop_entries.push((crate::ast::PropertyKey::Ident(name.clone()), prop_value));
            }
            JsxProp::Spread(_) => {
                // Spread is handled specially - skip for now
                // Full spread support would require Object.assign semantics
            }
        }
    }

    Ok(prop_entries)
}

/// Convert JSX tag name to string
fn tag_name_to_string(tag: &JsxTagName) -> String {
    match tag {
        JsxTagName::Ident(name) => name.clone(),
        JsxTagName::Member { object, property } => format!("{}.{}", object, property),
        JsxTagName::Namespaced { namespace, name } => format!("{}:{}", namespace, name),
    }
}

/// Build arguments from JSX children
fn build_jsx_children_args(children: &[JsxChild]) -> Vec<Expression> {
    let mut args = Vec::new();

    for child in children {
        match child {
            JsxChild::Text(s) => args.push(Expression::String(s.clone())),
            JsxChild::Expression(expr) => args.push(expr.clone()),
            JsxChild::Spread(_) => {}
            JsxChild::Element(jsx_expr) => args.push((**jsx_expr).clone()),
        }
    }

    args
}
