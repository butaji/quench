//! JSX element tests

use crate::transpile::tests::spec_jsx::helpers::{assert_codegen_not_empty, assert_jsx_parses};
use crate::transpile::hir::{JSXChild, JSXName};

#[test]
fn html_element_self_closing() {
    let jsx = assert_jsx_parses(r#"const x = <div />;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
    assert!(jsx.opening.self_closing);
    assert!(jsx.closing.is_none());
    assert_codegen_not_empty(&jsx);
}

#[test]
fn html_element_with_closing_tag() {
    let jsx = assert_jsx_parses(r#"const x = <div></div>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
    assert!(!jsx.opening.self_closing);
    assert!(jsx.closing.is_some());
    assert_codegen_not_empty(&jsx);
}

#[test]
fn html_element_img_self_closing() {
    let jsx = assert_jsx_parses(r#"const x = <img />;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "img"));
    assert!(jsx.opening.self_closing);
    assert_codegen_not_empty(&jsx);
}

#[test]
fn html_element_input_self_closing() {
    let jsx = assert_jsx_parses(r#"const x = <input />;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "input"));
    assert!(jsx.opening.self_closing);
    assert_codegen_not_empty(&jsx);
}

#[test]
fn html_element_span() {
    let jsx = assert_jsx_parses(r#"const x = <span></span>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "span"));
    assert_codegen_not_empty(&jsx);
}

#[test]
fn html_element_p() {
    let jsx = assert_jsx_parses(r#"const x = <p>paragraph</p>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "p"));
    assert!(!jsx.children.is_empty());
    assert_codegen_not_empty(&jsx);
}

#[test]
fn component_counter() {
    let jsx = assert_jsx_parses(r#"const x = <Counter />;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "Counter"));
    assert!(jsx.opening.self_closing);
    assert_codegen_not_empty(&jsx);
}

#[test]
fn component_my_component() {
    let jsx = assert_jsx_parses(r#"const x = <MyComponent />;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "MyComponent"));
    assert_codegen_not_empty(&jsx);
}

#[test]
fn component_with_closing_tag() {
    let jsx = assert_jsx_parses(r#"const x = <MyComponent></MyComponent>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "MyComponent"));
    assert!(!jsx.opening.self_closing);
    assert_codegen_not_empty(&jsx);
}

#[test]
fn component_member_expression() {
    let jsx = assert_jsx_parses(r#"const x = <React.Foo />;"#);
    assert!(matches!(jsx.opening.name, JSXName::Member { object: ref o, property: ref p } if o == "React" && p == "Foo"));
    assert_codegen_not_empty(&jsx);
}

#[test]
fn fragment_empty() {
    let jsx = assert_jsx_parses(r#"const x = <></>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Fragment));
    assert!(!jsx.opening.self_closing);
    assert!(jsx.closing.is_none());
    assert_codegen_not_empty(&jsx);
}

#[test]
fn fragment_with_content() {
    let jsx = assert_jsx_parses(r#"const x = <>hello</>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Fragment));
    assert!(!jsx.children.is_empty());
    assert_codegen_not_empty(&jsx);
}

#[test]
fn fragment_explicit_tag() {
    let jsx = assert_jsx_parses(r#"const x = <Fragment></Fragment>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "Fragment"));
    assert_codegen_not_empty(&jsx);
}

#[test]
fn nested_simple() {
    let jsx = assert_jsx_parses(r#"const x = <div><span>text</span></div>;"#);
    assert!(matches!(jsx.opening.name, JSXName::Ident(ref n) if n == "div"));
    assert!(!jsx.children.is_empty());
    let child = &jsx.children[0];
    assert!(matches!(child, JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
    assert_codegen_not_empty(&jsx);
}

#[test]
fn nested_deep() {
    let jsx = assert_jsx_parses(r#"const x = <div><span><em>deep</em></span></div>;"#);
    assert!(!jsx.children.is_empty());
    let child = &jsx.children[0];
    assert!(matches!(child, JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
    assert_codegen_not_empty(&jsx);
}

#[test]
fn nested_multiple_children() {
    let jsx = assert_jsx_parses(r#"const x = <div><span /><p /></div>;"#);
    assert_eq!(jsx.children.len(), 2);
    assert!(matches!(&jsx.children[0], JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "span")));
    assert!(matches!(&jsx.children[1], JSXChild::JSX(inner) if matches!(inner.opening.name, JSXName::Ident(ref n) if n == "p")));
    assert_codegen_not_empty(&jsx);
}
