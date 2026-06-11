//! Quench TSX/TS Compiler using esbuild
//!
//! Pipeline:
//! 1. esbuild strips TypeScript and transforms JSX to JS
//! 2. Transform imports and API calls

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Compile TSX/TS source to Quench-compatible JavaScript
pub fn compile_tsx(source: &str, _filename: &str) -> Result<String> {
    compile_with_esbuild(source, "tsx")
}

/// Compile TS/JS source
pub fn compile_ts(source: &str, _filename: &str) -> Result<String> {
    compile_with_esbuild(source, "ts")
}

fn compile_with_esbuild(source: &str, loader: &str) -> Result<String> {
    let raw_js = run_esbuild(source, loader)?;
    let transformed = transform_js(&raw_js)?;
    Ok(prepend_shims(&transformed))
}

fn run_esbuild(source: &str, loader: &str) -> Result<String> {
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

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn transform_js(js: &str) -> Result<String> {
    // Extract ink imports to know what to prefix
    let ink_imports = extract_ink_imports(js);

    // Step 1: Remove react/ink imports
    let js = strip_imports(js);

    // Step 2: Transform React.createElement calls
    let js = js
        .replace("React.createElement(React.Fragment,", "ink.Fragment(")
        .replace("React.createElement(Fragment,", "ink.Fragment(")
        .replace("React.createElement(", "ink.createElement(");

    // Step 3: Prefix ink imports in createElement calls
    let js = prefix_components(&js, &ink_imports);

    // Step 4: Prefix hooks
    let js = prefix_hooks(&js);

    // Step 5: Replace bare render() with ink.render()
    let js = js
        .replace("ink.render", "__protected_render")
        .replace("render(", "ink.render(")
        .replace("__protected_render", "ink.render");

    // Step 6: Transform dynamic node: imports to shim
    let js = js
        .replace(r#"import("node:readline")"#, r#"__tb_import("node:readline")"#)
        .replace("import('node:readline')", "__tb_import('node:readline')")
        .replace(r#"import("readline")"#, r#"__tb_import("readline")"#)
        .replace("import('readline')", "__tb_import('readline')");

    // Step 7: Process polyfills
    let js = js
        .replace("process.exit(", "ink.useApp().exit(")
        .replace("process.env.NODE_ENV", "\"production\"");

    Ok(js)
}

fn extract_ink_imports(js: &str) -> Vec<String> {
    js.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("import ") && (trimmed.contains("from \"ink\"") || trimmed.contains("from 'ink'"))
        })
        .flat_map(|line| extract_names_from_import(line))
        .collect()
}

fn extract_names_from_import(line: &str) -> Vec<String> {
    let start = match line.find('{') {
        Some(i) => i + 1,
        None => return vec![],
    };
    let end = match line.find('}') {
        Some(i) => i,
        None => return vec![],
    };
    line[start..end]
        .split(',')
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && n != "React")
        .collect()
}

fn prefix_components(js: &str, imports: &[String]) -> String {
    let mut result = js.to_string();
    for name in imports {
        // Protect already-prefixed
        result = result.replace(&format!("ink.ink.{}", name), &format!("ink.{}", name));
        // Prefix in createElement calls
        let from = format!("ink.createElement({}", name);
        let to = format!("ink.createElement(ink.{}", name);
        result = result.replace(&from, &to);
    }
    // Clean up
    result.replace("ink.ink.", "ink.")
}

fn prefix_hooks(js: &str) -> String {
    // All React hooks + Ink hooks that can be called as functions
    let hooks = [
        // React hooks
        "useState", "useEffect", "useRef", "useMemo", "useCallback",
        "useContext", "useReducer", "useLayoutEffect", "useImperativeHandle",
        "useDebugValue",
        // Ink hooks
        "useInput", "useApp", "useStdin", "useStdout", "useStderr",
        "useFocus", "useFocusManager", "useWindowSize", "useAnimation",
        "usePaste", "useCursor", "useBoxMetrics", "useIsScreenReaderEnabled",
        "measureElement", "useBridge", "createContext",
    ];
    let mut result = js.to_string();
    for hook in &hooks {
        // Protect already-prefixed
        result = result.replace(&format!("ink.ink.{}", hook), &format!("ink.{}", hook));
        // Prefix call sites
        result = result.replace(hook, &format!("ink.{}", hook));
    }
    result.replace("ink.ink.", "ink.")
}

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

// SHIMS constant
static SHIMS: &str = r#"// Quench Node.js/React shims
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
    if (/^f\d+$/.test(name)) return name;
    return name;
}
// Readline shim: override __tb_dispatch_key to also call readline handlers
// The runtime.js version reads from globals, so we need to hook into that flow
(function() {
    const __tb_orig_dispatch_key = globalThis.__tb_dispatch_key;
    globalThis.__tb_dispatch_key = function() {
        // Call the original (reads globals, calls inputHandlers)
        if (__tb_orig_dispatch_key) __tb_orig_dispatch_key();
        
        // Also call readline handlers
        const key = globalThis.__pending_key;
        const ctrl = globalThis.__pending_ctrl;
        const shift = globalThis.__pending_shift;
        const alt = globalThis.__pending_alt;
        const meta = globalThis.__pending_meta;
        const name = __tb_to_readline_name(key);
        const str = name.length === 1 ? name : '';
        const keyObj = { ctrl: ctrl, meta: meta, shift: shift, name: name, sequence: name.length === 1 ? name : '' };
        for (const handler of __tb_keypress_handlers) {
            try { 
                handler(str, keyObj); 
            } catch(e) { 
                console.error('[shim] Handler error:', e); 
            }
        }
    };
})();
const __tb_import_sync = function(moduleName) {
    const isStr = typeof moduleName === 'string';
    const name = isStr ? moduleName.replace('node:', '') : '';
    if (name === 'readline') { return __tb_readline_shim; }
    return {};
};
globalThis.__tb_import = function(moduleName) {
    const result = __tb_import_sync(moduleName);
    return { then: function(onFulfilled) { onFulfilled(result); }, catch: function() { return this; } };
};
globalThis.process = {
    exit: (code) => { try { ink.useApp().exit(); } catch(e) {} },
    stdout: {
      write: (s) => { try { ink.stdout_write(s); } catch(e) {} },
      get rows() { try { return ink.useStdout().rows; } catch(e) { return 24; } },
      get columns() { try { return ink.useStdout().columns; } catch(e) { return 80; } },
      isTTY: true
    },
    stderr: { write: (s) => { try { ink.stderr_write(s); } catch(e) {} } },
    stdin: {
      isTTY: true,
      setRawMode: () => {},
      resume: () => {},
      on: () => {},
      removeListener: () => {},
    },
    env: { NODE_ENV: 'production' },
    on: (evt, cb) => {},
};

"#;

fn prepend_shims(js: &str) -> String {
    let mut result = js.to_string();
    result.insert_str(0, SHIMS);
    result
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
