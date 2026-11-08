use super::{syntax_valid, syntax_valid_whole as valid_whole};
use oxc_span::SourceType;

#[test]
fn lazy_syntax_check_agrees_with_the_whole_program() {
    let sources = [
        "function f() { return 1 }",
        "function f() { let a; let a; }",
        "function f() { var x = 010; }",
        "'use strict'; function f() { var x = 010; }",
        "function f() { 'use strict'; with (a) {} }",
        "class C { m() { function g() { with (a) {} } } }",
        "var g = function () { return arguments }; function h(a, a) {}",
        "'use strict'; function h(a, a) {}",
        "function f() { function g() { break; } }",
        "function f() { yield: 1 } function* g() { function h() { var yield; } }",
        "(function () { function f() { return } })()",
        "function f() { import.meta }",
        "function f() { return \"\\u0061\" } let s = '\\u0062'",
        "function f() { await: 1 }",
        "class C { #p = 1; m() { return function () { return this.#p } } }",
        "class C { m() { return function () { return this.#q } } }",
    ];
    for source in sources {
        for source_type in [SourceType::cjs(), SourceType::mjs(), SourceType::script()] {
            assert_eq!(
                syntax_valid(source, source_type),
                valid_whole(source, source_type),
                "{source_type:?}: {source}"
            );
        }
    }
}

/// Compares the two checks on every `.js`, `.cjs` and `.mjs` file under
/// `QUENCH_SYNTAX_CORPUS`, as scripts, CommonJS and modules.
#[test]
#[ignore = "reads a corpus named by QUENCH_SYNTAX_CORPUS"]
fn lazy_syntax_check_agrees_on_a_corpus() {
    let root = std::env::var_os("QUENCH_SYNTAX_CORPUS").expect("QUENCH_SYNTAX_CORPUS");
    let mut pending = vec![std::path::PathBuf::from(root)];
    let (mut files, mut differ) = (0usize, Vec::new());
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
            continue;
        }
        if !path.extension().is_some_and(|extension| matches!(extension.to_str(), Some("js" | "cjs" | "mjs"))) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else { continue };
        files += 1;
        for source_type in [SourceType::cjs(), SourceType::mjs()] {
            if syntax_valid(&source, source_type) != valid_whole(&source, source_type) {
                differ.push(format!("{source_type:?} {}", path.display()));
            }
        }
    }
    eprintln!("files={files} differ={}", differ.len());
    assert!(differ.is_empty(), "{differ:#?}");
}
