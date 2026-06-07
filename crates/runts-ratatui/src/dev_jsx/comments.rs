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

pub fn is_string_start(chars: &[char], i: usize) -> bool {
    i < chars.len() && (chars[i] == '"' || chars[i] == '\'')
}

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

pub fn is_line_comment(chars: &[char], i: usize) -> bool {
    i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '/'
}

pub fn skip_line_comment(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i += 1;
    i
}

pub fn is_block_comment_start(chars: &[char], i: usize) -> bool {
    i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*'
}

pub fn skip_block_comment(chars: &[char], mut i: usize) -> usize {
    i += 2;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '/' { return i + 2; }
        i += 1;
    }
    chars.len()
}

/// Transform `import { ... } from 'ink'` lines:
/// - Component imports (Box, Text, etc.) are stripped
/// - Hook imports (useAnimation, useBoxMetrics, measureElement, etc.)
///   are replaced with references to runts_ink_hooks
pub fn transform_imports(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == 'i' && i + 6 < chars.len() && &chars[i..i + 7].iter().collect::<String>()[..] == "import " {
            let j = skip_import_line(&chars, i + 7);
            let import_line: String = chars[i..j].iter().collect();
            if let Some(replacement) = transform_ink_import(&import_line) {
                out.push_str(&replacement);
            }
            i = j;
        } else { out.push(chars[i]); i += 1; }
    }
    out
}

fn transform_ink_import(import_line: &str) -> Option<String> {
    // Only transform imports from 'ink'
    if !import_line.contains("'ink'") && !import_line.contains('"'.to_string().as_str()) {
        return None;
    }
    
    // Extract named imports
    let import_str = import_line.strip_prefix("import")?.trim();
    let (imports_part, _) = import_str.split_once("from")?;
    let imports_part = imports_part.trim().trim_start_matches('{').trim_end_matches('}').trim();
    if imports_part.is_empty() { return None; }
    
    let names: Vec<&str> = imports_part.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let mut replacements = Vec::new();
    
    for name in names {
        let import_name = name.trim();
        // Skip component types (handled by JSX lowering)
        if is_ink_component(import_name) { continue; }
        // Hooks and functions become runts_ink_hooks references
        replacements.push(format!("var {} = runts_ink_hooks.{}", import_name, import_name));
    }
    
    if replacements.is_empty() { None } else { Some(replacements.join(";\n")) }
}

fn is_ink_component(name: &str) -> bool {
    matches!(name, "Box" | "Text" | "Newline" | "Spacer" | "Static" | "Transform")
}

fn skip_import_line(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i] != '\n' { i += 1; }
    if i < chars.len() { i += 1; }
    i
}
