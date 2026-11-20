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

/// The free names each lazily parsed function has, as the parser's skim finds them or as the
/// semantic check of its tree does; `None` when a body failed its check.
fn lazy_free_names(source: &str, skim: bool) -> Option<Vec<(u32, Vec<String>)>> {
    let allocator = super::arena(source);
    let stubbing = crate::compile::program_shape::Stubbing { functions: true, arrows: false, skim };
    let facts = std::cell::RefCell::new(super::BodyFacts::deferring(stubbing, false));
    let mut check = |body: oxc_parser::LazyBody<'_, '_>| facts.borrow_mut().check(body, &[]);
    let mut skimmer = |body: oxc_parser::SkimmedBody<'_>| facts.borrow_mut().skim(body, source);
    // SAFETY: both live through the parse, which alone calls them.
    let (check, skim) = unsafe { (oxc_parser::LazyBodyCheck::new(&mut check), skim.then(|| oxc_parser::LazyBodySkim::new(&mut skimmer))) };
    let parsed = super::parse(&allocator, source, SourceType::script(), Some(0), Some(check), skim, false, false, crate::compile::annex_b_targets::Recognize::No, None, None);
    assert!(parsed.diagnostics.is_empty(), "{source}");
    let facts = facts.into_inner();
    let mut found: Vec<(u32, Vec<String>)> = facts.free_names.iter().map(|(start, names)| (*start, names.iter().map(|name| name.to_string()).collect())).collect();
    found.sort_unstable();
    (!facts.failed).then_some(found)
}

#[test]
fn skimmed_bodies_have_the_free_names_of_their_semantic_check() {
    let sources = [
        "function f(a, b) { return a + b + c; }",
        "function f(a) { var b = a; { let c = b; return c + d; } }",
        "function f() { return g(h, function (i) { return i + j; }); }",
        "function f() { try { x(); } catch (e) { return e + y; } finally { z; } }",
        "function f() { for (let i = 0; i < n; i++) { var k = i; } for (const k of ks) { use(k); } return k; }",
        "function f() { switch (x) { case 1: let a = 1; break; default: return a + b; } }",
        "function f() { label: for (;;) { break label; } return arguments.length; }",
        "function f() { return () => this + u + (v => v + w)(1); }",
        "function f() { class A extends B { m() { return super.m() + C; } static s() { return A; } } return new A; }",
        "function f() { return { a, b: c, [d]: e, get g() { return h; }, set g(v) { i = v; }, m() { return n; } }; }",
        "function f() { var { a, b: [c, d = e], ...r } = o; return a + c + d + r + p; }",
        "function f() { function g() { return q; } return g() + r; }",
        "function f() { return typeof t + (u in v) + (w instanceof X); }",
        "function f() { x = `${y}${`${z}`}`; return tag`${aa}`; }",
        "function f() { function* g() { yield h; yield* i; } return g; }",
        "var f = function (a) { return a + b; };",
        "function f(a) { 'use strict'; return this + a + b; }",
        "function f() { return /re/g.test(s) ? t : u; }",
        "function f() { eval('x'); }",
        "function f() { with (o) { return p; } }",
        "function f() { return a?.b?.[c]?.(d) ?? e; }",
        "function f() { return [x, ...y, , z]; }",
        "function f() { if (a) return b; else if (c) return d; do { e(); } while (g); while (h) i(); }",
        "function f() { return new.target; }",
        "function f(f) { return f + g; }",
        "function g() { return g; } function h() { return g; }",
        "function f(s) { /* with eval */ return s.startsWith('eval') + o.evaluate(w); }",
    ];
    for source in sources {
        assert_eq!(lazy_free_names(source, true), lazy_free_names(source, false), "{source}");
    }
}

/// Each `.js` file under `QUENCH_SYNTAX_CORPUS`, as the body of one function, has the free
/// names, and is accepted or not, as its semantic check says.
#[test]
#[ignore = "reads a corpus named by QUENCH_SYNTAX_CORPUS"]
fn skimmed_bodies_match_the_semantic_check_on_a_corpus() {
    let root = std::env::var_os("QUENCH_SYNTAX_CORPUS").expect("QUENCH_SYNTAX_CORPUS");
    let mut pending = vec![std::path::PathBuf::from(root)];
    let (mut files, mut differ) = (0usize, Vec::new());
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "js") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let source = format!("function w() {{\n{text}\n}}");
        if !valid_whole(&source, SourceType::script()) {
            continue;
        }
        files += 1;
        if lazy_free_names(&source, true) != lazy_free_names(&source, false) {
            differ.push(path.display().to_string());
        }
    }
    eprintln!("files={files} differ={}", differ.len());
    assert!(differ.is_empty(), "{differ:#?}");
}

/// The words `eval` and `with` in comments, strings and longer names leave a body's free names
/// bounded: a bundle's module that holds them still compiles on first call. A real `eval` call
/// or `with` statement, here or in a function inside, does not.
#[test]
fn scope_words_outside_code_keep_free_names() {
    let names = lazy_free_names("function f(s) { // with eval\n return s.startsWith('eval') + x.evaluate(y); }", false).expect("checked");
    assert_eq!(names.iter().map(|(_, names)| names.clone()).collect::<Vec<_>>(), vec![vec!["x".to_string(), "y".to_string()]]);
    for reaching in [
        "function f() { return eval(s); }",
        "function f() { with (o) { return p; } }",
        "function f() { return function () { return eval(s); }; }",
        "function f() { return () => { with (o) return p; }; }",
    ] {
        let names = lazy_free_names(reaching, false).expect("checked");
        assert!(names.iter().all(|(start, _)| *start != 0), "{reaching}: {names:?}");
    }
}
