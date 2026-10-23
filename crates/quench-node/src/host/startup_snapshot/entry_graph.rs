//! Which snapshot a file entry restores (`SnapshotKind`): the bootstrap alone when nothing
//! its files say can load a module beyond them, and the preloaded builtins otherwise.
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
/// `module.require`, `createRequire`), `process.getBuiltinModule`, and `console`, whose first
/// use loads Node's console and inspection modules, which the preloaded builtins hold (a file
/// printing one line: 22.9 ms on the bootstrap, 15.8 ms on the preloaded builtins). A match
/// anywhere, a comment or string included, counts.
const MODULE_LOADING_NAMES: &[&str] = &["require", "getBuiltinModule", "console"];
/// The files the scan reads before the run starts. A larger own graph is a program whose own
/// loading outweighs what restoring the builtins costs it.
const SCANNED_FILES: usize = 64;
/// The bytes the scan reads and parses before the run starts, as for `SCANNED_FILES`.
const SCANNED_BYTES: usize = 1 << 20;

/// The snapshot kind the process whose entry is `entry` restores.
pub(super) fn snapshot_kind(entry: &Path) -> SnapshotKind {
    let mut scan = Scan {
        pending: vec![entry.to_owned()],
        seen: HashSet::new(),
        bytes: 0,
    };
    match scan.run() {
        Some(()) => SnapshotKind::Bootstrap,
        None => SnapshotKind::Preloaded,
    }
}

struct Scan {
    pending: Vec<PathBuf>,
    seen: HashSet<PathBuf>,
    bytes: usize,
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
                _ => source_specifiers(std::str::from_utf8(&bytes).ok()?)?,
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

/// The snapshot kind a process whose entry is the inline source `source` (`-e`, `-p`)
/// restores, by the rule `snapshot_kind` applies to a file: the bootstrap alone when it names
/// no module and no way of loading one.
pub(super) fn inline_snapshot_kind(source: &str) -> SnapshotKind {
    match source_specifiers(source) {
        Some(specifiers) if specifiers.is_empty() => SnapshotKind::Bootstrap,
        _ => SnapshotKind::Preloaded,
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
        assert_eq!(snapshot_kind(&root.join("main.mjs")), SnapshotKind::Bootstrap);
        assert_eq!(snapshot_kind(&root.join("a.wasm")), SnapshotKind::Bootstrap);
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
            assert_eq!(snapshot_kind(&root.join(entry)), SnapshotKind::Preloaded, "{entry}");
        }
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
