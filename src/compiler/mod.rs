//! Quench TSX/TS Compiler using esbuild
//!
//! Pipeline:
//! 1. esbuild strips TypeScript and transforms JSX to JS
//! 2. Post-process to add Quench shims and fix imports

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// React hooks that need to be prefixed with ink.
const REACT_HOOKS: &[&str] = &[
    "useState",
    "useEffect",
    "useRef",
    "useMemo",
    "useCallback",
    "useContext",
    "useReducer",
    "useLayoutEffect",
    "useImperativeHandle",
    "useDebugValue",
];

/// Compile TSX/TS source to Quench-compatible JavaScript
pub fn compile_tsx(source: &str, _filename: &str) -> Result<String> {
    compile_with_esbuild(source, "tsx")
}

/// Compile TS/JS source
pub fn compile_ts(source: &str, _filename: &str) -> Result<String> {
    compile_with_esbuild(source, "ts")
}

fn compile_with_esbuild(source: &str, loader: &str) -> Result<String> {
    let temp_input = format!("/tmp/quench_input_{}.tsx", std::process::id());
    std::fs::write(&temp_input, source)?;

    let output = Command::new("npx")
        .args(&[
            "esbuild",
            &temp_input,
            "--outfile=/dev/stdout",
            &format!("--loader:.tsx={}", loader),
            &format!("--loader:.ts={}", loader),
            "--format=esm",
        ])
        .output()
        .context("Failed to run esbuild")?;

    let _ = std::fs::remove_file(&temp_input);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("esbuild failed: {}", err));
    }

    let js = String::from_utf8_lossy(&output.stdout).to_string();
    let js = strip_imports(&js);
    let js = transform_node_imports(&js);
    let js = transform_react_apis(&js);
    let js = transform_react_hooks(&js);
    let js = transform_process_polyfills(&js);
    Ok(prepend_shims(&js))
}

fn transform_react_apis(js: &str) -> String {
    js.replace("React.createElement(React.Fragment,", "ink.Fragment(")
        .replace("React.createElement(Fragment,", "ink.Fragment(")
        .replace("React.createElement", "ink.createElement")
}

fn transform_react_hooks(js: &str) -> String {
    let mut result = js.to_string();
    for hook in REACT_HOOKS {
        let replacement = format!("ink.{}", hook);
        result = result.replace(hook, &replacement);
        result = result.replace(&format!("ink.ink.{}", hook), &replacement);
    }
    result
}

fn transform_process_polyfills(js: &str) -> String {
    let result = transform_exit_calls(js);
    let result = transform_std_streams(&result);
    transform_env_and_signals(&result)
}

fn transform_exit_calls(js: &str) -> String {
    let re_exit = regex::Regex::new(r"process\.exit\([^)]*\)").unwrap();
    re_exit.replace_all(js, "ink.useApp().exit()").to_string()
}

fn transform_std_streams(js: &str) -> String {
    // We deliberately leave `process.stdout.rows` and `process.stdout.columns`
    // untouched so the runtime polyfill returns the *real* terminal size from
    // the Rust bridge.  Hardcoding them to 100 made the root `<Box height={...}>`
    // overflow the visible area.
    let r = js.replace("process.stdin?.isTTY", "true");
    let r = r.replace("process.stdin.setRawMode", "(() => {})");
    let r = r.replace("process.stdin.resume", "(() => {})");
    r.replace("process.stdin.on", "(() => {})")
}

fn transform_env_and_signals(js: &str) -> String {
    let re_env = regex::Regex::new(r"process\.env\.(\w+)").unwrap();
    let step1 = js.replace("process.env.NODE_ENV", "\"production\"");
    let step2 = step1.replace("process.on(\"SIGINT\",", "// SIGINT handled by quench\n// ");
    let step3 = step2.replace("process.on('SIGINT',", "// SIGINT handled by quench\n// ");
    re_env.replace_all(&step3, "undefined").to_string()
}

fn prepend_shims(js: &str) -> String {
    static SHIMS: &str = r#"// Quench Node.js/React shims
// Readline shim with working keypress events
const __tb_keypress_handlers = [];
const __tb_readline_shim = {
    createInterface: (options) => {
        const inputObj = {
            _handlers: {},
            on: function(evt, handler) {
                if (evt === 'keypress') {
                    this._handlers.keypress = this._handlers.keypress || [];
                    this._handlers.keypress.push(handler);
                    __tb_keypress_handlers.push(handler);
                }
            },
            removeListener: function(evt, handler) {
                if (evt === 'keypress' && this._handlers.keypress) {
                    this._handlers.keypress = this._handlers.keypress.filter(h => h !== handler);
                    __tb_keypress_handlers = __tb_keypress_handlers.filter(h => h !== handler);
                }
            },
        };
        return {
            input: inputObj,
            output: { write: () => {} },
            question: (q, cb) => { if (typeof cb === 'function') cb(''); },
            on: () => {},
            close: () => {},
        };
    },
};
// Override __tb_dispatch_key to also call readline handlers.  The Rust side
// emits Ink-style names (e.g. "upArrow", "downArrow", "return", "escape"),
// but `node:readline` uses Node-style names ("up", "down", "return", "escape",
// "tab", "backspace").  Translate so user code that wires up `readline` keeps
// working unchanged.
const __tb_readline_key_aliases = {
    upArrow: 'up', downArrow: 'down', leftArrow: 'left', rightArrow: 'right',
    pageUp: 'pageup', pageDown: 'pagedown',
    return: 'return', escape: 'escape', tab: 'tab', backspace: 'backspace',
    delete: 'delete', home: 'home', end: 'end', insert: 'insert',
};
function __tb_to_readline_name(name) {
    if (!name) return name;
    if (Object.prototype.hasOwnProperty.call(__tb_readline_key_aliases, name)) {
        return __tb_readline_key_aliases[name];
    }
    // F1-F12
    if (/^f\d+$/.test(name)) return name;
    // Fall back to the Ink name (e.g. "a", "b" for character keys).
    return name;
}
const __tb_orig_dispatch_key = globalThis.__tb_dispatch_key;
globalThis.__tb_dispatch_key = function(key, ctrl, shift, alt, meta) {
    if (__tb_orig_dispatch_key) __tb_orig_dispatch_key(key, ctrl, shift, alt, meta);
    const name = __tb_to_readline_name(key);
    const str = name.length === 1 ? name : '';
    const keyObj = {
        ctrl: ctrl,
        meta: meta,
        shift: shift,
        name: name,
        sequence: name.length === 1 ? name : '',
    };
    for (const handler of __tb_keypress_handlers) {
        try { handler(str, keyObj); } catch(e) { console.error('[shim] Handler error:', e); }
    }
};
// Non-async version for compatibility
const __tb_import_sync = function(moduleName) {
    const isStr = typeof moduleName === 'string';
    const name = isStr ? moduleName.replace('node:', '') : '';
    if (name === 'readline') { return __tb_readline_shim; }
    return {};
};
globalThis.__tb_import = function(moduleName) {
    // Return a resolved promise-like object
    const result = __tb_import_sync(moduleName);
    return {
        then: function(onFulfilled) { onFulfilled(result); },
        catch: function() { return this; }
    };
};

globalThis.process = {
    exit: (code) => { try { ink.useApp().exit(); } catch(e) {} },
    stdout: {
      write: (s) => { try { ink.stdout_write(s); } catch(e) {} },
      get rows() { try { return ink.useStdout().stdout.rows; } catch(e) { return 24; } },
      get columns() { try { return ink.useStdout().stdout.columns; } catch(e) { return 80; } },
      isTTY: true
    },
    stderr: { write: (s) => { try { ink.stderr_write(s); } catch(e) {} } },
    stdin: { isTTY: true, setRawMode: () => {}, resume: () => {}, on: () => {} },
    env: { NODE_ENV: 'production' },
    on: (evt, cb) => {},
};

"#;
    let mut result = js.to_string();
    result.insert_str(0, SHIMS);
    result
}

/// Remove import statements for react and ink
fn strip_imports(js: &str) -> String {
    js.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                let from_react = trimmed.contains("from \"react\"") || trimmed.contains("from 'react'");
                let from_ink = trimmed.contains("from \"ink\"") || trimmed.contains("from 'ink'");
                return !(from_react || from_ink);
            }
            if trimmed.starts_with("import type ") {
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Transform node:* dynamic imports to use our shim
fn transform_node_imports(js: &str) -> String {
    js.replace(r#"import("node:readline")"#, r#"__tb_import("node:readline")"#)
        .replace(r#"import('node:readline')"#, r#"__tb_import('node:readline')"#)
        .replace(r#"import("readline")"#, r#"__tb_import("readline")"#)
        .replace(r#"import('readline')"#, r#"__tb_import('readline')"#)
}

/// Compile a file
pub fn compile_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {:?}", path))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.tsx");

    if filename.ends_with(".tsx") || filename.ends_with(".jsx") {
        compile_tsx(&source, filename)
    } else {
        compile_ts(&source, filename)
    }
}
