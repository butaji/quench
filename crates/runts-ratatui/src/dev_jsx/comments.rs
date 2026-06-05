//! Comment stripping functions for JSX transformation.

/// Strip line and block comments from a string
/// without touching comments inside string literals.
pub fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if is_string_start(&chars, i) { i = copy_string_literal(&chars, i, &mut out); }
        else if chars[i] == '`' { i = copy_template_literal(&chars, i, &mut out); }
        else if is_line_comment(&chars, i) { i = skip_line_comment(&chars, i); }
        else if is_block_comment_start(&chars, i) { i = skip_block_comment(&chars, i); }
        else { out.push(chars[i]); i += 1; }
    }
    out
}

pub fn is_string_start(chars: &[char], i: usize) -> bool { i < chars.len() && (chars[i] == '"' || chars[i] == '\'') }

pub fn copy_string_literal(chars: &[char], mut i: usize, out: &mut String) -> usize {
    let quote = chars[i]; out.push(chars[i]); i += 1;
    while i < chars.len() && chars[i] != quote {
        if chars[i] == '\\' && i + 1 < chars.len() { out.push(chars[i]); out.push(chars[i + 1]); i += 2; }
        else { out.push(chars[i]); i += 1; }
    }
    if i < chars.len() { out.push(chars[i]); i += 1; }
    i
}

pub fn copy_template_literal(chars: &[char], mut i: usize, out: &mut String) -> usize {
    out.push(chars[i]); i += 1;
    while i < chars.len() {
        if chars[i] == '`' { out.push(chars[i]); i += 1; break; }
        if chars[i] == '\\' && i + 1 < chars.len() { out.push(chars[i]); out.push(chars[i + 1]); i += 2; }
        else { out.push(chars[i]); i += 1; }
    }
    i
}

pub fn is_line_comment(chars: &[char], i: usize) -> bool { i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '/' }

pub fn skip_line_comment(chars: &[char], mut i: usize) -> usize { while i < chars.len() && chars[i] != '\n' { i += 1; } i += 1; i }

pub fn is_block_comment_start(chars: &[char], i: usize) -> bool { i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' }

pub fn skip_block_comment(chars: &[char], mut i: usize) -> usize {
    i += 2;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '/' { return i + 2; }
        i += 1;
    }
    chars.len()
}

/// Strip `import { ink }` lines so rquickjs doesn't
/// try to load the ink package at eval time.
pub fn strip_imports(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == 'i' && i + 6 < chars.len() && &chars[i..i + 7].iter().collect::<String>()[..] == "import " {
            let j = skip_import_line(&chars, i + 7);
            i = j;
        } else { out.push(chars[i]); i += 1; }
    }
    out
}

fn skip_import_line(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i] != '\n' { i += 1; }
    if i < chars.len() { i += 1; }
    i
}
