//! The `//# name=value` directives of a script's source (`sourceURL`, `sourceMappingURL`)
//! that `vm.Script` reports: the last comment of that form that begins its line.

/// What opens a line comment.
const COMMENT: &str = "//";
/// What follows the opener in a directive: `#`, or the legacy `@`.
const MARKERS: [char; 2] = ['#', '@'];

/// The value of the last `//# name=value` directive in `source` that begins its line, after
/// whitespace. A source that never names the directive is rejected by one substring search;
/// any other is read line by line from the end, each byte once: only a line's first comment
/// opener can begin it, so a long line of many `//` (URLs in a minified bundle) costs one pass,
/// not a search back to its start per opener (json-stringify-inspector: 94% of its run).
pub(super) fn value<'s>(source: &'s str, name: &str) -> Option<&'s str> {
    if !source.contains(name) {
        return None;
    }
    source.rsplit('\n').find_map(|line| directive(line, name))
}

/// The directive value of `line` when it is, after whitespace, a comment whose text is
/// `#name=value` with a value of no whitespace.
fn directive<'s>(line: &'s str, name: &str) -> Option<&'s str> {
    let value = line
        .trim_start()
        .strip_prefix(COMMENT)?
        .strip_prefix(MARKERS)?
        .trim_start()
        .strip_prefix(name)?
        .trim_start()
        .strip_prefix('=')?
        .trim();
    (!value.is_empty() && !value.contains(char::is_whitespace)).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::value;

    #[test]
    fn the_last_directive_that_begins_its_line_wins() {
        let source = "a();\n//# sourceURL=first.js\n  //@ sourceURL = second.js \r\nb(); //# sourceURL=inline.js\n";
        assert_eq!(value(source, "sourceURL"), Some("second.js"));
        assert_eq!(value(source, "sourceMappingURL"), None);
        assert_eq!(
            value("//# sourceMappingURL=x.map", "sourceMappingURL"),
            Some("x.map")
        );
    }

    #[test]
    fn malformed_directives_report_nothing() {
        for source in [
            "///# sourceURL=a",
            "// //# sourceURL=a",
            "//# sourceURL=",
            "//# sourceURL=a b",
            "//#sourceURLx=a",
            "x //# sourceURL=a",
        ] {
            assert_eq!(value(source, "sourceURL"), None, "{source:?}");
        }
        assert_eq!(
            value("\t//#\tsourceURL\t=\ta.js", "sourceURL"),
            Some("a.js")
        );
    }
}
