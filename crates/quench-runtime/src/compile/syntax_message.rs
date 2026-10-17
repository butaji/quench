//! V8's SyntaxError text for a parse failure (src/common/message-template.h). V8 names the
//! token it could not accept by its kind; a lexical failure has a fixed message. OXC owns
//! the syntax, so its diagnostic supplies the failure's kind and position.
use oxc_diagnostics::OxcDiagnostic;

/// `kInvalidOrUnexpectedToken`: V8's report for an ILLEGAL token.
const ILLEGAL_TOKEN: &str = "Invalid or unexpected token";

/// OXC lexer failures and V8's message for each; any other lexical failure is an ILLEGAL
/// token.
const LEXICAL_FAILURES: &[(&str, Option<&str>)] = &[
    ("Invalid Character", None),
    ("Unterminated string", None),
    ("Unterminated multiline comment", None),
    ("Unterminated template", Some("Unterminated template literal")),
    ("Unterminated regular expression", Some("Invalid regular expression: missing /")),
    ("Invalid escape sequence", Some("Invalid hexadecimal escape sequence")),
    ("Invalid Unicode escape sequence", Some("Invalid Unicode escape sequence")),
    ("Invalid characters after number", None),
    ("Invalid Number", None),
];

/// OXC parser failures that mean "this token cannot appear here", which V8 reports by
/// naming the token.
const UNEXPECTED_TOKEN_FAILURES: &[&str] = &[
    "Unexpected token",
    "Expected a semicolon or an implicit semicolon after a statement, but found none",
    "Unexpected end of input",
];

/// Keywords V8 lexes as their own tokens, so `Unexpected token 'kw'` (ECMA-262 ReservedWord).
const RESERVED_WORDS: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do",
    "else", "enum", "export", "extends", "false", "finally", "for", "function", "if", "import", "in",
    "instanceof", "new", "null", "return", "super", "switch", "this", "throw", "true", "try",
    "typeof", "var", "void", "while", "with",
];

/// ECMA-262 punctuators, longest first so the first match is the maximal munch.
const PUNCTUATORS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "<<", ">>", "**", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%", "&",
    "|", "^", "!", "~", "?", ":", "=", ".",
];

/// V8's message for an OXC parse diagnostic over `source`; other failures keep OXC's text.
pub(crate) fn parse_failure(source: &str, error: &OxcDiagnostic) -> String {
    let message = error.message.as_ref();
    if let Some((_, v8)) = LEXICAL_FAILURES.iter().find(|(oxc, _)| message.starts_with(oxc)) {
        return v8.unwrap_or(ILLEGAL_TOKEN).to_owned();
    }
    let unexpected = UNEXPECTED_TOKEN_FAILURES.contains(&message)
        || (message.starts_with("Expected `") && message.contains("` but found `"));
    if !unexpected {
        return message.to_owned();
    }
    let offset = error.labels.first().map_or(source.len(), |label| usize::try_from(label.offset()).unwrap_or(source.len()));
    let before = source.get(..offset).unwrap_or_default();
    if follows_decorator(before) {
        return ILLEGAL_TOKEN.to_owned();
    }
    unexpected_token(source.get(offset..).unwrap_or_default())
}

/// Whether `before` holds a decorator sigil outside strings, templates and comments. OXC
/// parses decorators and fails later, but V8 has none and fails at the `@` itself; once a
/// parse has failed, a `@` in code position can only be that sigil.
fn follows_decorator(before: &str) -> bool {
    let mut characters = before.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            DECORATOR => return true,
            '\'' | '"' | '`' => {
                let mut escaped = false;
                for inner in characters.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if inner == '\\' {
                        escaped = true;
                    } else if inner == character {
                        break;
                    }
                }
            }
            '/' if characters.peek() == Some(&'/') => {
                for inner in characters.by_ref() {
                    if oxc_syntax::line_terminator::is_line_terminator(inner) {
                        break;
                    }
                }
            }
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                let mut star = false;
                for inner in characters.by_ref() {
                    if star && inner == '/' {
                        break;
                    }
                    star = inner == '*';
                }
            }
            _ => {}
        }
    }
    false
}

/// The decorator sigil, an ILLEGAL character to V8.
const DECORATOR: char = '@';

/// `Unexpected ...` for the token that starts `rest` after whitespace and comments.
fn unexpected_token(rest: &str) -> String {
    let rest = skip_trivia(rest);
    let Some(first) = rest.chars().next() else {
        return "Unexpected end of input".to_owned();
    };
    if oxc_syntax::identifier::is_identifier_start(first) {
        let end = rest
            .char_indices()
            .find(|(_, character)| !oxc_syntax::identifier::is_identifier_part(*character))
            .map_or(rest.len(), |(index, _)| index);
        let word = &rest[..end];
        return if RESERVED_WORDS.contains(&word) {
            format!("Unexpected token '{word}'")
        } else {
            format!("Unexpected identifier '{word}'")
        };
    }
    let second = rest[first.len_utf8()..].chars().next();
    match first {
        '0'..='9' => return "Unexpected number".to_owned(),
        '.' if second.is_some_and(|character| character.is_ascii_digit()) => return "Unexpected number".to_owned(),
        '"' | '\'' => return "Unexpected string".to_owned(),
        '`' => return "Unexpected template string".to_owned(),
        _ => {}
    }
    match PUNCTUATORS.iter().find(|punctuator| rest.starts_with(*punctuator)) {
        Some(punctuator) => format!("Unexpected token '{punctuator}'"),
        None => ILLEGAL_TOKEN.to_owned(),
    }
}

/// `rest` after ECMAScript white space, line terminators and comments.
fn skip_trivia(mut rest: &str) -> &str {
    loop {
        let trimmed = rest.trim_start_matches(|character: char| {
            oxc_syntax::identifier::is_white_space(character) || oxc_syntax::line_terminator::is_line_terminator(character)
        });
        if let Some(comment) = trimmed.strip_prefix("//") {
            rest = comment.find(oxc_syntax::line_terminator::is_line_terminator).map_or("", |end| &comment[end..]);
        } else if let Some(comment) = trimmed.strip_prefix("/*") {
            match comment.find("*/") {
                Some(end) => rest = &comment[end + "*/".len()..],
                None => return "",
            }
        } else {
            return trimmed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unexpected_token;

    #[test]
    fn tokens_are_named_like_v8() {
        assert_eq!(unexpected_token(" License"), "Unexpected identifier 'License'");
        assert_eq!(unexpected_token("1"), "Unexpected number");
        assert_eq!(unexpected_token(""), "Unexpected end of input");
        assert_eq!(unexpected_token("}"), "Unexpected token '}'");
        assert_eq!(unexpected_token("/* c */ var"), "Unexpected token 'var'");
        assert_eq!(unexpected_token("@import"), "Invalid or unexpected token");
        assert_eq!(unexpected_token(">>>= 1"), "Unexpected token '>>>='");
        assert_eq!(unexpected_token("'s'"), "Unexpected string");
    }

    #[test]
    fn decorators_fail_at_their_sigil() {
        assert!(super::follows_decorator("@layer "));
        assert!(super::follows_decorator("x;\n@theme default "));
        assert!(!super::follows_decorator("a.b 'x@y' // @z\n /* @w */ "));
    }
}
