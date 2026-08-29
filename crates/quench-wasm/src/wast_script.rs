//! Score each wast directive: validator directives decide, execute-class fail.

use crate::decode::{
    features_for_path, inspect_quote_with, invalid_branch_hint_target, ModuleStatus,
};
use crate::legacy_try::unfold_if_legacy;
use crate::wast_exec::{self, Store};
use wasmparser::WasmFeatures;
use wast::lexer::Lexer;
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastDirective};

/// One scored wast directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveResult {
    pub line: usize,
    pub kind: String,
    pub passed: bool,
    pub expected: String,
    pub got: String,
}

impl DirectiveResult {
    /// `file:line kind: expected …; got …`
    pub fn format_line(&self, file: &str) -> String {
        format!(
            "{file}:{} {}: expected {}; got {}",
            self.line, self.kind, self.expected, self.got
        )
    }
}

/// Per-file directive scores.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WastReport {
    pub results: Vec<DirectiveResult>,
}

impl WastReport {
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| !r.passed).count()
    }
}

/// Parse `source` as wast and score every directive using features for `filename`.
pub fn run_wast(filename: &str, source: &str) -> WastReport {
    let source = unfold_if_legacy(filename, source);
    let source = source.as_ref();
    let features = features_for_path(filename);
    let mut lexer = Lexer::new(source);
    lexer.allow_confusing_unicode(true);
    let buf = match ParseBuffer::new_with_lexer(lexer) {
        Ok(buf) => buf,
        Err(error) => return parse_failure(error.to_string()),
    };
    let mut script: Wast<'_> = match parser::parse(&buf) {
        Ok(script) => script,
        Err(error) => return parse_failure(error.to_string()),
    };

    let mut results = Vec::with_capacity(script.directives.len());
    let mut store = Store::new();
    for directive in &mut script.directives {
        let line = line_of(directive.span(), source);
        results.push(score_directive(line, directive, features, &mut store));
    }
    WastReport { results }
}

fn parse_failure(got: String) -> WastReport {
    WastReport {
        results: vec![DirectiveResult {
            line: 1,
            kind: "wast".to_string(),
            passed: false,
            expected: "parse".to_string(),
            got,
        }],
    }
}

fn line_of(span: wast::token::Span, source: &str) -> usize {
    span.linecol_in(source).0 + 1
}

fn score_directive(
    line: usize,
    directive: &mut WastDirective<'_>,
    features: WasmFeatures,
    store: &mut Store,
) -> DirectiveResult {
    match directive {
        WastDirective::AssertMalformed {
            module, message, ..
        } => score_malformed(line, module, message, features),
        WastDirective::AssertMalformedCustom {
            module, message, ..
        } => score_malformed(line, module, message, features),
        WastDirective::AssertInvalid {
            module, message, ..
        } => score_invalid(line, module, message, features),
        WastDirective::AssertInvalidCustom {
            module, message, ..
        } => score_invalid(line, module, message, features),
        WastDirective::Module(module) => score_valid_module(line, module, features, store, true),
        WastDirective::ModuleDefinition(module) => {
            score_valid_module(line, module, features, store, false)
        }
        WastDirective::AssertReturn { exec, results, .. } => {
            wast_exec::score_return(line, exec, results, store, features)
        }
        WastDirective::AssertTrap { exec, message, .. } => {
            wast_exec::score_trap(line, exec, message, store, features)
        }
        WastDirective::Invoke(invoke) => wast_exec::score_invoke(line, invoke, store),
        WastDirective::AssertExhaustion { call, message, .. } => {
            wast_exec::score_exhaustion(line, call, message, store)
        }
        WastDirective::AssertException { exec, .. } => {
            wast_exec::score_exception(line, exec, store, features)
        }
        WastDirective::AssertUnlinkable {
            module, message, ..
        } => wast_exec::score_unlinkable(line, module, message, features, store),
        WastDirective::Register { name, module, .. } => {
            store.register(name, module.map(|id| id.name()));
            DirectiveResult {
                line,
                kind: "register".to_string(),
                passed: true,
                expected: "register".to_string(),
                got: "ok".to_string(),
            }
        }
        other => unimplemented_directive(line, other),
    }
}

fn score_malformed(
    line: usize,
    module: &mut QuoteWat<'_>,
    expected_message: &str,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_malformed".to_string();
    let expected = format!("malformed ({expected_message})");
    match inspect_quote_with(module, features) {
        ModuleStatus::ParseError(got) | ModuleStatus::ValidateError(got) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got,
        },
        ModuleStatus::Valid => DirectiveResult {
            line,
            kind,
            passed: false,
            expected,
            got: "valid".to_string(),
        },
    }
}

fn score_invalid(
    line: usize,
    module: &mut QuoteWat<'_>,
    expected_message: &str,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_invalid".to_string();
    let expected = format!("invalid ({expected_message})");
    if let Some(got) = invalid_branch_hint_target(module) {
        return DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got,
        };
    }
    match inspect_quote_with(module, features) {
        ModuleStatus::ValidateError(got) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got,
        },
        ModuleStatus::ParseError(got) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got: format!("malformed: {got}"),
        },
        ModuleStatus::Valid => DirectiveResult {
            line,
            kind,
            passed: false,
            expected,
            got: "valid".to_string(),
        },
    }
}

fn score_valid_module(
    line: usize,
    module: &mut QuoteWat<'_>,
    features: WasmFeatures,
    store: &mut Store,
    instantiate: bool,
) -> DirectiveResult {
    let kind = "module".to_string();
    let expected = "valid".to_string();
    match inspect_quote_with(module, features) {
        ModuleStatus::Valid => {
            if instantiate {
                store.instantiate_quote(module, features);
            }
            DirectiveResult {
                line,
                kind,
                passed: true,
                expected,
                got: "valid".to_string(),
            }
        }
        ModuleStatus::ParseError(got) => DirectiveResult {
            line,
            kind,
            passed: false,
            expected,
            got: format!("malformed: {got}"),
        },
        ModuleStatus::ValidateError(got) => DirectiveResult {
            line,
            kind,
            passed: false,
            expected,
            got: format!("invalid: {got}"),
        },
    }
}

fn unimplemented_directive(line: usize, directive: &WastDirective<'_>) -> DirectiveResult {
    let kind = directive_kind(directive).to_string();
    DirectiveResult {
        line,
        kind: kind.clone(),
        passed: false,
        expected: kind,
        got: "unimplemented".to_string(),
    }
}

fn directive_kind(directive: &WastDirective<'_>) -> &'static str {
    match directive {
        WastDirective::Module(_) | WastDirective::ModuleDefinition(_) => "module",
        WastDirective::ModuleInstance { .. } => "module_instance",
        WastDirective::AssertMalformed { .. } | WastDirective::AssertMalformedCustom { .. } => {
            "assert_malformed"
        }
        WastDirective::AssertInvalid { .. } | WastDirective::AssertInvalidCustom { .. } => {
            "assert_invalid"
        }
        WastDirective::Register { .. } => "register",
        WastDirective::Invoke(_) => "invoke",
        WastDirective::AssertTrap { .. } => "assert_trap",
        WastDirective::AssertReturn { .. } => "assert_return",
        WastDirective::AssertExhaustion { .. } => "assert_exhaustion",
        WastDirective::AssertUnlinkable { .. } => "assert_unlinkable",
        WastDirective::AssertException { .. } => "assert_exception",
        WastDirective::AssertSuspension { .. } => "assert_suspension",
        WastDirective::Thread(_) => "thread",
        WastDirective::Wait { .. } => "wait",
    }
}

#[cfg(test)]
mod tests {
    use super::run_wast;
    use crate::decode::{inspect_binary, ModuleStatus};

    #[test]
    fn assert_malformed_binary_passes() {
        let source = r#"(assert_malformed (module binary "") "unexpected end")"#;
        let report = run_wast("malformed.wast", source);
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].passed, "{:?}", report.results[0]);
        assert_eq!(report.results[0].kind, "assert_malformed");
    }

    #[test]
    fn assert_malformed_quote_passes() {
        let source = r#"(assert_malformed (module quote "(module") "unexpected eof")"#;
        let report = run_wast("quote.wast", source);
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].passed, "{:?}", report.results[0]);
    }

    #[test]
    fn assert_malformed_text_passes() {
        let source = r#"(assert_malformed (module binary "\00as") "unexpected end")"#;
        let report = run_wast("text-bin.wast", source);
        assert!(report.results[0].passed, "{:?}", report.results[0]);
    }

    #[test]
    fn assert_invalid_passes() {
        let source = r#"
(assert_invalid
  (module (func (unreachable) (drop (local.get 0))))
  "unknown local")
"#;
        let report = run_wast("invalid.wast", source);
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].passed, "{:?}", report.results[0]);
        assert_eq!(report.results[0].kind, "assert_invalid");
    }

    #[test]
    fn unlinkable_incompatible_func_type() {
        let report = run_wast(
            "unlink.wast",
            r#"
(module
  (import "spectest" "print_i32" (func $f (param i32)))
  (export "print" (func $f))
)
(register "reexport_f")
(assert_unlinkable
  (module (import "reexport_f" "print" (func (param i64))))
  "incompatible import type")
"#,
        );
        assert!(
            report.results.iter().all(|r| r.passed),
            "{:?}",
            report.results
        );
    }

    #[test]
    fn global_get_and_extended_const_execute() {
        let report = run_wast(
            "global-init.wast",
            r#"
(module
  (global (import "spectest" "global_i32") i32)
  (global $z (export "z") i32 (i32.add (global.get 0) (i32.const 42)))
  (func (export "get-z") (result i32) (global.get $z))
)
(assert_return (invoke "get-z") (i32.const 708))
(assert_return (get "z") (i32.const 708))
"#,
        );
        assert!(
            report.results.iter().all(|r| r.passed),
            "{:?}",
            report.results
        );
    }

    #[test]
    fn valid_module_accepted_without_running() {
        let source = r#"(module (func (export "answer") (result i32) i32.const 42))"#;
        let report = run_wast("module.wast", source);
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].passed, "{:?}", report.results[0]);
        assert_eq!(report.results[0].kind, "module");
    }

    #[test]
    fn br_if_in_void_block() {
        let report = run_wast(
            "brif.wast",
            r#"
(module
  (func (export "t")
    (block (drop (i32.ctz (br_if 0 (i32.const 0) (i32.const 1)))))
  )
  (func (export "v") (result i32)
    (block (result i32) (i32.ctz (br_if 0 (i32.const 1) (i32.const 1))))
  )
)
(assert_return (invoke "t"))
(assert_return (invoke "v") (i32.const 1))
"#,
        );
        assert!(
            report.results.iter().all(|r| r.passed),
            "{:?}",
            report.results
        );
    }

    #[test]
    fn invoke_and_assert_return_run_i32() {
        let source = r#"
(module (func (export "answer") (result i32) i32.const 42))
(assert_return (invoke "answer") (i32.const 42))
(invoke "answer")
"#;
        let report = run_wast("exec.wast", source);
        assert_eq!(report.results.len(), 3);
        assert!(
            report.results.iter().all(|r| r.passed),
            "{:?}",
            report.results
        );
        assert_eq!(report.results[1].kind, "assert_return");
        assert_eq!(report.results[2].kind, "invoke");
    }

    #[test]
    fn inspect_binary_classifies_empty_as_parse_error() {
        match inspect_binary(b"") {
            ModuleStatus::ParseError(_) => {}
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn inspect_binary_accepts_empty_module() {
        let bytes = wat_bytes("(module)");
        assert_eq!(inspect_binary(&bytes), ModuleStatus::Valid);
    }

    #[test]
    fn confusing_unicode_is_lexed_not_a_file_parse_failure() {
        let source = "(module (func (export \"\u{202e}krow\") (result i32) i32.const 1))\n";
        let report = run_wast("names.wast", source);
        assert_eq!(report.results.len(), 1, "{:?}", report.results);
        assert_eq!(report.results[0].kind, "module");
        assert!(report.results[0].passed, "{:?}", report.results[0]);
    }

    fn wat_bytes(source: &str) -> Vec<u8> {
        let buf = wast::parser::ParseBuffer::new(source).expect("buf");
        let mut wat: wast::Wat<'_> = wast::parser::parse(&buf).expect("wat");
        wat.encode().expect("encode")
    }
}
