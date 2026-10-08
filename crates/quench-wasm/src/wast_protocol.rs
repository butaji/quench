//! Shared WAST execution protocol and directive scoring.

use std::borrow::Cow;

use quench_runtime::WasmValue;
use wasmparser::WasmFeatures;
use wast::{WastExecute, WastInvoke, WastRet, Wat};

use crate::shared_wast::Store;
use crate::wast_script::DirectiveResult;

pub(crate) enum Outcome<V> {
    Values(Vec<V>),
    Trap(Cow<'static, str>),
    Exception,
    Unimplemented(String),
    Missing,
}

pub(crate) enum LinkError {
    Exception,
    Unlinkable(String),
    Trap(String),
    Unsupported(String),
}

pub(crate) fn score_return(
    line: usize,
    exec: &mut WastExecute<'_>,
    expected: &[WastRet<'_>],
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_return".to_string();
    match store.run(exec, features) {
        Outcome::Unimplemented(got) => unimplemented(line, &kind, &got),
        Outcome::Values(got) => compare_rets(line, kind, expected, &got, store),
        Outcome::Trap(got) => fail(line, kind, "return", &got),
        Outcome::Exception => fail(line, kind, "return", "exception"),
        Outcome::Missing => fail(line, kind, "return", "unknown export"),
    }
}

pub(crate) fn score_trap(
    line: usize,
    exec: &mut WastExecute<'_>,
    message: &str,
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_trap".to_string();
    let expected = format!("trap ({message})");
    match store.run(exec, features) {
        Outcome::Unimplemented(got) => unimplemented(line, &kind, &got),
        Outcome::Trap(got) if trap_matches(&got, message) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got: got.to_string(),
        },
        Outcome::Trap(got) => fail(line, kind, &expected, &got),
        Outcome::Values(_) => fail(line, kind, &expected, "return"),
        Outcome::Exception => fail(line, kind, &expected, "exception"),
        Outcome::Missing => fail(line, kind, &expected, "unknown export"),
    }
}

pub(crate) fn score_exception(
    line: usize,
    exec: &mut WastExecute<'_>,
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_exception".to_string();
    match store.run(exec, features) {
        Outcome::Exception => DirectiveResult {
            line,
            kind,
            passed: true,
            expected: "exception".to_string(),
            got: "exception".to_string(),
        },
        Outcome::Unimplemented(got) => unimplemented(line, &kind, &got),
        Outcome::Trap(got) => fail(line, kind, "exception", &got),
        Outcome::Values(_) => fail(line, kind, "exception", "return"),
        Outcome::Missing => fail(line, kind, "exception", "unknown export"),
    }
}

pub(crate) fn score_unlinkable(
    line: usize,
    module: &mut Wat<'_>,
    message: &str,
    features: WasmFeatures,
    store: &mut Store,
) -> DirectiveResult {
    let kind = "assert_unlinkable".to_string();
    let expected = format!("unlinkable ({message})");
    let bytes = match module.encode() {
        Ok(bytes) => bytes,
        Err(error) => return fail(line, kind, &expected, &error.to_string()),
    };
    match store.try_link(&bytes, features) {
        Err(LinkError::Unlinkable(got)) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got,
        },
        Err(LinkError::Unsupported(got)) => fail(line, kind, &expected, &got),
        Err(LinkError::Trap(got)) => fail(line, kind, &expected, &got),
        Err(LinkError::Exception) => fail(line, kind, &expected, "exception"),
        Ok(()) => fail(line, kind, &expected, "linked"),
    }
}

pub(crate) fn score_exhaustion(
    line: usize,
    invoke: &WastInvoke<'_>,
    message: &str,
    store: &mut Store,
) -> DirectiveResult {
    let kind = "assert_exhaustion".to_string();
    let expected = format!("exhaustion ({message})");
    match store.invoke(invoke) {
        Outcome::Unimplemented(got) => unimplemented(line, &kind, &got),
        Outcome::Trap(got) if trap_matches(&got, message) || got.contains("exhaust") => {
            DirectiveResult {
                line,
                kind,
                passed: true,
                expected,
                got: got.to_string(),
            }
        }
        Outcome::Trap(got) => fail(line, kind, &expected, &got),
        Outcome::Values(_) => fail(line, kind, &expected, "return"),
        Outcome::Exception => fail(line, kind, &expected, "exception"),
        Outcome::Missing => fail(line, kind, &expected, "unknown export"),
    }
}

pub(crate) fn score_invoke(
    line: usize,
    invoke: &WastInvoke<'_>,
    store: &mut Store,
) -> DirectiveResult {
    let kind = "invoke".to_string();
    match store.invoke(invoke) {
        Outcome::Unimplemented(got) => unimplemented(line, &kind, &got),
        Outcome::Values(_) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected: "invoke".to_string(),
            got: "ok".to_string(),
        },
        Outcome::Trap(got) => fail(line, kind, "invoke", &got),
        Outcome::Exception => fail(line, kind, "invoke", "exception"),
        Outcome::Missing => fail(line, kind, "invoke", "unknown export"),
    }
}

fn compare_rets(
    line: usize,
    kind: String,
    expected: &[WastRet<'_>],
    got: &[WasmValue],
    store: &Store,
) -> DirectiveResult {
    if expected.len() != got.len() {
        return fail(
            line,
            kind,
            &format!("{} values", expected.len()),
            &format!("{} values", got.len()),
        );
    }
    for (want, got) in expected.iter().zip(got) {
        if !matches!(want, WastRet::Core(want) if store.matches_result(got, want)) {
            return fail(line, kind, &format!("{want:?}"), &format!("{got:?}"));
        }
    }
    DirectiveResult {
        line,
        kind,
        passed: true,
        expected: "match".to_string(),
        got: "match".to_string(),
    }
}

fn trap_matches(got: &str, expected: &str) -> bool {
    got.contains(expected) || expected.contains(got)
}

fn unimplemented(line: usize, kind: &str, got: &str) -> DirectiveResult {
    DirectiveResult {
        line,
        kind: kind.to_string(),
        passed: false,
        expected: kind.to_string(),
        got: got.to_string(),
    }
}

fn fail(line: usize, kind: String, expected: &str, got: &str) -> DirectiveResult {
    DirectiveResult {
        line,
        kind,
        passed: false,
        expected: expected.to_string(),
        got: got.to_string(),
    }
}
