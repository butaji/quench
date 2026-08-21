//! Compile-time syntax validation for JavaScript embedded in Rust string
//! literals. `checked_js!(r#"...")` expands to the same `&'static str`, but
//! fails the build with the parser's diagnostic when the source is not a
//! syntactically valid JavaScript script.

use proc_macro::TokenStream;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use syn::{parse_macro_input, LitStr};

#[proc_macro]
pub fn checked_js(input: TokenStream) -> TokenStream {
    let original = input.clone();
    let literal = parse_macro_input!(input as LitStr);
    let source = literal.value();

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &source, SourceType::cjs()).parse();
    if !parsed.errors.is_empty() {
        let messages = parsed
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        return syn::Error::new(literal.span(), format!("invalid JavaScript: {messages}"))
            .to_compile_error()
            .into();
    }

    original
}
