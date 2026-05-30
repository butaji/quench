//! End-to-end test for TSX parsing → Fresh codegen pipeline.
//!
//! This test demonstrates the Fresh JSX → VNode codegen pipeline.
//!
//! Note: The transpiler (src/transpile/) lives in the main `runts` crate,
//! not in `runts_lib` (which is `crates/runts-lib/`). Integration tests
//! cannot access the transpiler directly.
//!
//! This test focuses on the Fresh codegen functions which ARE accessible
//! via `runts_fresh::{jsx_element, jsx_text, page_component}`.

use runts_fresh::{jsx_element, jsx_text, page_component};
use runts_plugin::Plugin;
use quote::quote;

fn normalize(s: &str) -> String {
    // TokenStream::to_string() adds spaces around :: and other tokens
    // Normalize to match expected patterns
    let s = s.replace(" :: ", "::");
    let s = s.replace(" ::", "::");
    let s = s.replace(":: ", "::");
    let s = s.replace(" (", "(");
    let s = s.replace("( ", "(");
    let s = s.replace(" )", ")");
    s
}

#[test]
fn test_jsx_element_codegen() {
    // Test the jsx_element function directly
    // Simulates what would be generated from: <div class="test">Hello World</div>
    let attrs = vec![("class".to_string(), quote! { "test" })];
    let children = vec![jsx_text("Hello World")];
    let element = jsx_element("div", attrs, children);
    let s = normalize(&element.to_string());
    eprintln!("DEBUG test_jsx_element: normalized s = {:?}", s);

    assert!(s.contains("VNode::Element"), "Should contain VNode::Element\nGot: {}", s);
    assert!(s.contains("\"div\""), "Should contain div tag\nGot: {}", s);
    assert!(s.contains("\"class\""), "Should contain class attribute\nGot: {}", s);
    assert!(s.contains("Hello World"), "Should contain text child\nGot: {}", s);

    println!("jsx_element output:\n{}", s);
}

#[test]
fn test_jsx_text_codegen() {
    let text = jsx_text("Welcome to runts!");
    let s = normalize(&text.to_string());

    assert!(s.contains("VNode::Text"), "Should contain VNode::Text\nGot: {}", s);
    assert!(s.contains("Welcome to runts!"), "Should contain text\nGot: {}", s);

    println!("jsx_text output:\n{}", s);
}

#[test]
fn test_jsx_fragment_codegen() {
    use runts_fresh::jsx_fragment;

    let children = vec![jsx_text("Part 1"), jsx_text("Part 2")];
    let fragment = jsx_fragment(children);
    let s = normalize(&fragment.to_string());

    assert!(s.contains("VNode::Fragment"), "Should contain VNode::Fragment\nGot: {}", s);

    println!("jsx_fragment output:\n{}", s);
}

#[test]
fn test_page_component_codegen() {
    // Simulates what would be generated from a TSX component like:
    // export default function Home() { return <div>Hello</div>; }
    let body = jsx_element("div", vec![], vec![jsx_text("Hello")]);
    let page = page_component("Home", body);
    let s = normalize(&page.to_string());

    assert!(s.contains("pub fn Home"), "Should contain function declaration\nGot: {}", s);
    assert!(s.contains("runts_lib::runtime::vdom::VNode"), "Should return VNode\nGot: {}", s);
    assert!(s.contains("VNode::Element"), "Should contain VNode\nGot: {}", s);

    println!("page_component output:\n{}", s);
}

#[test]
fn test_nested_element_codegen() {
    // Simulates: <div class="home"><h1>{greeting}</h1></div>
    let h1_element = jsx_element("h1", vec![], vec![jsx_text("Welcome to runts!")]);
    let div_element = jsx_element(
        "div",
        vec![("class".to_string(), quote! { "home" })],
        vec![h1_element],
    );
    let page = page_component("Home", div_element);
    let s = normalize(&page.to_string());

    assert!(s.contains("\"div\""), "Should contain div\nGot: {}", s);
    assert!(s.contains("\"h1\""), "Should contain h1\nGot: {}", s);
    assert!(s.contains("\"class\""), "Should contain class\nGot: {}", s);
    assert!(s.contains("Welcome to runts!"), "Should contain greeting\nGot: {}", s);

    println!("Nested element output:\n{}", s);
}

#[test]
fn test_fresh_plugin_returns_string() {
    // Verify the plugin API works (returns a string)
    let plugin = runts_fresh::FreshPlugin;
    let result = plugin.codegen_module("{}");
    assert!(result.is_ok(), "codegen_module should return Ok");
    let code = result.unwrap();
    assert!(!code.is_empty(), "codegen_module should return non-empty string");

    // The stub returns a generic module comment
    assert!(code.contains("auto-generated"), "Stub should return generated code\nGot: {}", code);

    println!("FreshPlugin stub output:\n{}", code);
}

#[test]
fn test_attr_value_no_literal_quotes() {
    // Verify that attr values don't contain escaped literal quotes
    // Input: quote! { "test" } should produce AttrValue::String("test") not AttrValue::String("\"test\"")
    let attrs = vec![("class".to_string(), quote! { "test" })];
    let children = vec![jsx_text("Hello")];
    let element = jsx_element("div", attrs, children);
    let s = normalize(&element.to_string());
    eprintln!("DEBUG test_attr_value_no_literal_quotes: normalized s = {:?}", s);

    // The attr value should be "test" (as a string literal in generated code)
    // NOT "\"test\"" (a string containing literal quote characters)
    // We check that the generated code does NOT contain the escaped form
    assert!(!s.contains(r#""\"test\"""#), 
        "Attr value should not contain escaped literal quotes. Got: {}", s);
    
    // It should contain the unescaped string literal "test"
    // The generated code should look like: AttrValue::from("test")
    // which is valid Rust and the string value is just "test" without extra quotes
    assert!(normalize(&s).contains(r#"AttrValue::from("test")"#),
        "Attr value should be AttrValue::from(\"test\"). Got: {}", s);
}
