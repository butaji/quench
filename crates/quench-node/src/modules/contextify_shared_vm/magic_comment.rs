//! The `//# name=value` directives of a script's source (`sourceURL`, `sourceMappingURL`)
//! that `vm.Script` reports: the last comment of that form that begins its line.

/// What opens a line comment.
const COMMENT: &str = "//";
/// What follows the opener in a directive: `#`, or the legacy `@`.
const MARKERS: [char; 2] = ['#', '@'];

/// The value of the last `//# name=value` directive in `source` that begins its line, after
/// whitespace. The search jumps between comment openers from the end, so a script of many
/// lines and few comments costs what its comments do, not a pass over every line.
pub(super) fn value<'s>(source: &'s str, name: &str) -> Option<&'s str> {
    source
        .rmatch_indices(COMMENT)
        .find_map(|(at, _)| directive(source, at, name))
}

/// The directive value of the comment opening at `at`, when only whitespace precedes it on
/// its line and its text is `#name=value` with a value of no whitespace.
fn directive<'s>(source: &'s str, at: usize, name: &str) -> Option<&'s str> {
    let line_start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
    if !source[line_start..at].trim_start().is_empty() {
        return None;
    }
    let rest = &source[at + COMMENT.len()..];
    let comment = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let value = comment
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
