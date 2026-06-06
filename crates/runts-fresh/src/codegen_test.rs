//! Tests for codegen module

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(s: &str) -> String {
        let s = s.replace(" :: ", "::");
        let s = s.replace(" ::", "::");
        let s = s.replace(":: ", "::");
        s
    }

    #[test]
    fn test_jsx_element() {
        let attrs = vec![("class".into(), quote! { "home" })];
        let children = vec![crate::codegen::jsx_text("Hello")];
        let result = crate::codegen::jsx_element("div", attrs, children);
        let s = normalize(&result.to_string());
        assert!(s.contains("VNode::Element"));
        assert!(s.contains("\"div\""));
        assert!(s.contains("\"class\""));
    }

    #[test]
    fn test_jsx_text() {
        let result = crate::codegen::jsx_text("Hello World");
        let s = normalize(&result.to_string());
        assert!(s.contains("VNode::Text"));
        assert!(s.contains("Hello World"));
    }

    #[test]
    fn test_jsx_fragment() {
        let children = vec![crate::codegen::jsx_text("a"), crate::codegen::jsx_text("b")];
        let result = crate::codegen::jsx_fragment(children);
        let s = normalize(&result.to_string());
        assert!(s.contains("VNode::Fragment"));
    }

    #[test]
    fn test_page_component() {
        let body = crate::codegen::jsx_element("div", vec![], vec![]);
        let result = crate::codegen::page_component("HomePage", body);
        let s = normalize(&result.to_string());
        assert!(s.contains("fn HomePage"));
        assert!(s.contains("runts_lib::runtime::vdom::VNode"));
    }
}
