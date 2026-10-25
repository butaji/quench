//! What a file entry's files say about the modules it loads (`EntryLoading`), which decides
//! whether a process restores a snapshot by default and which kind (`SnapshotKind`): the
//! bootstrap alone when nothing its files say can load a module beyond them, and the
//! preloaded builtins otherwise.
//!
//! The scan follows the entry's static graph (static imports and re-exports, and a WebAssembly
//! module's imports, resolved as the ES module loader resolves them) and stops at the first
//! sign of a builtin: a builtin specifier, a dynamic `import()`, or a name a file loads
//! modules through. Either kind runs the program the same way; the choice
//! only decides which builtins are already loaded, so whatever the scan cannot read, parse
//! or afford is `Preloaded`, the kind every process restored before the scan existed.

use super::SnapshotKind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Names through which a file loads modules no static import names: CommonJS `require` (and
/// `module.require`, `createRequire`) and `process.getBuiltinModule`. A match anywhere, a
/// comment or string included, counts.
const MODULE_LOADING_NAMES: &[&str] = &["require", "getBuiltinModule"];
/// `console`, whose first use loads Node's console and inspection modules, which the preloaded
/// builtins hold (a file printing one line: 22.9 ms on the bootstrap, 15.8 ms on the preloaded
/// builtins), though a process that boots without a snapshot loads only what it prints with.
const CONSOLE_NAME: &str = "console";
/// The keywords every static import, re-export and dynamic `import()` is written with: a file
/// without either names no module, and is not parsed.
const MODULE_SYNTAX: &[&str] = &["import", "export"];
/// The files the scan reads before the run starts. A larger own graph is a program whose own
/// loading outweighs what restoring the builtins costs it.
const SCANNED_FILES: usize = 64;
/// The bytes the scan reads and parses before the run starts, as for `SCANNED_FILES`.
const SCANNED_BYTES: usize = 1 << 20;

/// What an entry's files say about the modules it loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryLoading {
    /// Its static graph names only its own files and packages.
    SelfContained,
    /// As `SelfContained`, with a file naming `console` (`CONSOLE_NAME`).
    Console,
    /// It may load a builtin, or a module the scan cannot follow, read, parse or afford.
    Modules,
}

impl EntryLoading {
    /// The snapshot kind a process restores for an entry loading so.
    pub(super) fn snapshot_kind(self) -> SnapshotKind {
        match self {
            Self::SelfContained => SnapshotKind::Bootstrap,
            Self::Console | Self::Modules => SnapshotKind::Preloaded,
        }
    }
}

/// What the files of the graph whose entry is `entry` load.
pub(super) fn entry_loading(entry: &Path) -> EntryLoading {
    let mut scan = Scan {
        pending: vec![entry.to_owned()],
        seen: HashSet::new(),
        bytes: 0,
        console: false,
    };
    match scan.run() {
        None => EntryLoading::Modules,
        Some(()) if scan.console => EntryLoading::Console,
        Some(()) => EntryLoading::SelfContained,
    }
}

struct Scan {
    pending: Vec<PathBuf>,
    seen: HashSet<PathBuf>,
    bytes: usize,
    /// Whether a file names `console`.
    console: bool,
}

impl Scan {
    /// `Some` when every file of the graph names only files of the graph.
    fn run(&mut self) -> Option<()> {
        while let Some(path) = self.pending.pop() {
            if !self.seen.insert(path.clone()) {
                continue;
            }
            if self.seen.len() > SCANNED_FILES {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            self.bytes += bytes.len();
            if self.bytes > SCANNED_BYTES {
                return None;
            }
            let specifiers = match path.extension().and_then(|extension| extension.to_str()) {
                Some("json") => Vec::new(),
                Some("node") => return None,
                Some("wasm") => wasm_specifiers(&bytes)?,
                _ => {
                    let text = std::str::from_utf8(&bytes).ok()?;
                    self.console |= text.contains(CONSOLE_NAME);
                    source_specifiers(text)?
                }
            };
            let directory = path.parent()?;
            for specifier in specifiers {
                if crate::host::shared_vm::commonjs::is_builtin_specifier(&specifier) {
                    return None;
                }
                let resolved =
                    crate::host::shared_vm::with_import_resolver(|resolver| resolver.resolve(directory, &specifier))
                        .ok()?
                        .full_path();
                self.pending.push(std::fs::canonicalize(resolved).ok()?);
            }
        }
        Some(())
    }
}

/// What the inline source `source` (`-e`, `-p`) loads, by the rule `entry_loading` applies
/// to a file: self-contained when it names no module and no way of loading one.
pub(super) fn inline_entry_loading(source: &str) -> EntryLoading {
    match source_specifiers(source) {
        Some(specifiers) if !specifiers.is_empty() => EntryLoading::Modules,
        Some(_) if source.contains(CONSOLE_NAME) => EntryLoading::Console,
        Some(_) => EntryLoading::SelfContained,
        None => EntryLoading::Modules,
    }
}

/// The modules a WebAssembly module imports.
fn wasm_specifiers(bytes: &[u8]) -> Option<Vec<String>> {
    let interface = quench_wasm::interface(bytes)?;
    Some(interface.imports.into_iter().map(|import| import.module).collect())
}

/// The specifiers a JavaScript file's static imports and re-exports name; `None` when it may
/// load modules otherwise, or does not parse.
fn source_specifiers(text: &str) -> Option<Vec<String>> {
    if MODULE_LOADING_NAMES.iter().any(|name| text.contains(name)) {
        return None;
    }
    if !MODULE_SYNTAX.iter().any(|keyword| text.contains(keyword)) {
        return Some(Vec::new());
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::unambiguous()).parse();
    if !parsed.diagnostics.is_empty() || !parsed.module_record.dynamic_imports.is_empty() {
        return None;
    }
    Some(parsed.module_record.requested_modules.keys().map(|specifier| specifier.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of `files` (path, contents) for one case.
    fn graph(case: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("quench-entry-graph-{}-{case}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, contents) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        std::fs::canonicalize(&root).unwrap()
    }

    /// A WebAssembly module importing function `name` from `module`.
    fn wasm_importing(module: &str, name: &str) -> Vec<u8> {
        let mut entry = vec![module.len() as u8];
        entry.extend_from_slice(module.as_bytes());
        entry.push(name.len() as u8);
        entry.extend_from_slice(name.as_bytes());
        entry.extend_from_slice(&[0x00, 0x00]);
        let mut import_section = vec![0x01];
        import_section.extend(entry);
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend_from_slice(&[0x01, 0x04, 0x01, 0x60, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x02, import_section.len() as u8]);
        bytes.extend(import_section);
        bytes
    }

    #[test]
    fn own_files_and_packages_restore_the_bootstrap() {
        let wasm = wasm_importing("env", "clock");
        let root = graph(
            "own",
            &[
                ("main.mjs", b"import { run } from './a.wasm'; export * from './b.mjs'; run();"),
                ("a.wasm", &wasm),
                ("b.mjs", b"import data from './c.json' with { type: 'json' }; export { data };"),
                ("c.json", b"{}"),
                ("node_modules/env/package.json", br#"{ "type": "module", "exports": "./index.js" }"#),
                ("node_modules/env/index.js", b"export function clock() { return 0; }"),
            ],
        );
        assert_eq!(entry_loading(&root.join("main.mjs")), EntryLoading::SelfContained);
        assert_eq!(entry_loading(&root.join("a.wasm")), EntryLoading::SelfContained);
    }

    #[test]
    fn any_way_to_a_builtin_restores_them_preloaded() {
        let wasm = wasm_importing("node:fs", "readFileSync");
        let root = graph(
            "builtins",
            &[
                ("import.mjs", b"import './own.mjs'; import { readFileSync } from 'fs';"),
                ("own.mjs", b"export {};"),
                ("deep.mjs", b"import './builtin.mjs';"),
                ("builtin.mjs", b"export * from 'node:path';"),
                ("require.cjs", b"const fs = require('fs');"),
                ("dynamic.mjs", b"await import('./own.mjs');"),
                ("get.mjs", b"process.getBuiltinModule('fs');"),
                ("wasm.wasm", &wasm),
                ("addon.mjs", b"import './addon.node';"),
                ("addon.node", b""),
                ("missing.mjs", b"import 'not-installed';"),
                ("invalid.mjs", b"import"),
            ],
        );
        for entry in [
            "import.mjs", "deep.mjs", "require.cjs", "dynamic.mjs", "get.mjs", "wasm.wasm", "addon.mjs",
            "missing.mjs", "invalid.mjs",
        ] {
            assert_eq!(entry_loading(&root.join(entry)), EntryLoading::Modules, "{entry}");
        }
    }

    #[test]
    fn a_graph_naming_only_console_loads_no_module() {
        let root = graph(
            "console",
            &[
                ("script.js", b"let total = 0; for (const x of [1, 2]) total += x; console.log(total);"),
                ("main.mjs", b"import { value } from './value.mjs'; console.log(value);"),
                ("value.mjs", b"export const value = 1;"),
                ("quiet.mjs", b"export const value = 1;"),
            ],
        );
        assert_eq!(entry_loading(&root.join("script.js")), EntryLoading::Console);
        assert_eq!(entry_loading(&root.join("main.mjs")), EntryLoading::Console);
        assert_eq!(entry_loading(&root.join("quiet.mjs")), EntryLoading::SelfContained);
        assert_eq!(EntryLoading::Console.snapshot_kind(), SnapshotKind::Preloaded);
        assert_eq!(inline_entry_loading("console.log(42)"), EntryLoading::Console);
        assert_eq!(inline_entry_loading("1 + 1"), EntryLoading::SelfContained);
        assert_eq!(inline_entry_loading("require('fs')"), EntryLoading::Modules);
        assert_eq!(inline_entry_loading("import('node:fs')"), EntryLoading::Modules);
    }

    #[test]
    fn kinds_round_trip_through_their_file_names() {
        for kind in SnapshotKind::ALL {
            let path = PathBuf::from(format!("/cache/0123456789abcdef.{}.qss", kind.name()));
            assert_eq!(SnapshotKind::of_path(&path), Some(kind));
        }
        assert_eq!(SnapshotKind::of_path(Path::new("/cache/0123456789abcdef.qss")), None);
    }
}
