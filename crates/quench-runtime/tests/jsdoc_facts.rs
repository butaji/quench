//! JSDoc types are TypeScript types spelled in comments: a JavaScript function annotated with
//! JSDoc and its TypeScript twin give the same declared facts (parameters, returns, locals,
//! fields), read from the one type-to-fact source (`compile::typescript::jsdoc`).

use quench_runtime::{Engine, ResidualProgram};

/// Each function's declared facts, in function order: its parameters' language sets, and its
/// declared return (`DECLARED_ANY`'s bits where none), with every annotated local named in `locals`.
fn facts(program: &ResidualProgram, locals: &[&str]) -> Vec<(String, Vec<u16>, Option<u16>, Vec<Option<u16>>)> {
    let declared = program.declared_types.as_deref();
    (0..program.functions.len()).map(|index| {
        let function = program.function(index);
        let name = function.name.map(|atom| program.atoms.get(atom as usize).unwrap_or_default().to_string()).unwrap_or_default();
        let start = function.source_span.map(|span| span.start);
        let returns = declared.zip(start).and_then(|(declared, start)| declared.declared_signature(start)).map(|signature| signature.returns);
        let locals = locals.iter().map(|local| declared.and_then(|declared| declared.declared_local(start.unwrap_or(u32::MAX), local))).collect();
        (name, function.declared_parameters().to_vec(), returns, locals)
    }).collect()
}

fn same_facts(javascript: &str, typescript: &str, locals: &[&str]) {
    let js = Engine::specialize(javascript, "jsdoc.js").unwrap_or_else(|errors| panic!("{errors:?}"));
    let ts = Engine::specialize_typescript(typescript, "jsdoc.ts").unwrap_or_else(|errors| panic!("{errors:?}"));
    let (js_facts, ts_facts) = (facts(&js, locals), facts(&ts, locals));
    assert!(ts_facts.iter().any(|(_, parameters, returns, locals)| !parameters.is_empty() || returns.is_some() || locals.iter().any(Option::is_some)),
        "the TypeScript twin declares something: {ts_facts:?}");
    assert_eq!(js_facts, ts_facts, "JSDoc and TypeScript give the same facts");
}

#[test]
fn parameters_and_returns() {
    same_facts(
        "/**\n * @param {number} a\n * @param {string} b\n * @returns {number}\n */\nfunction f(a, b) { return a + b.length; }\nf(1, 'x');",
        "function f(a: number, b: string): number { return a + b.length; }\nf(1, 'x');",
        &[],
    );
}

#[test]
fn optional_nullable_and_closure_spellings() {
    same_facts(
        "/**\n * @param {?number} a\n * @param {boolean=} b\n * @param {number} [c]\n * @param {Array.<string>} d\n * @param {*} e\n */\nfunction f(a, d, e, b, c) { return a; }\nf(1);",
        "function f(a: (number) | null, d: Array<string>, e: any, b?: boolean, c?: number) { return a; }\nf(1);",
        &[],
    );
}

#[test]
fn arrows_methods_and_exported_declarations() {
    same_facts(
        "/** @param {number} x @returns {number} */\nconst g = (x) => x * 2;\nclass C {\n  /** @param {string} s */\n  m(s) { return s; }\n}\ng(2); new C().m('a');",
        "const g = (x: number): number => x * 2;\nclass C {\n  m(s: string) { return s; }\n}\ng(2); new C().m('a');",
        &[],
    );
}

#[test]
fn typed_locals_typedefs_and_casts() {
    same_facts(
        "/** @typedef {number} Count */\nfunction f() {\n  /** @type {Count} */\n  let n = 0;\n  const s = /** @type {string} */ (String(n));\n  return s.length + n;\n}\nf();",
        "type Count = number;\nfunction f() {\n  let n: Count = 0;\n  const s = (String(n)) as string;\n  return s.length + n;\n}\nf();",
        &["n", "s"],
    );
}

#[test]
fn template_names_stay_open() {
    same_facts(
        "/** @typedef {number} T */\n/**\n * @template T\n * @param {T} x\n * @param {number} y\n * @returns {T}\n */\nfunction f(x, y) { return x; }\nf(1, 2);",
        "type T = number;\nfunction f<T>(x: T, y: number): T { return x; }\nf(1, 2);",
        &[],
    );
}
