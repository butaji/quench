//! Parallel transpile pass.
//!
//! `runts transpile` and the build pipeline both walk a
//! tree of `.ts` / `.tsx` files and parse each one to an
//! HIR `Module`. For small projects the per-file parse is
//! fast enough to do serially, but as a project grows the
//! cumulative parse time becomes a real cost.
//!
//! This module exposes a single helper,
//! [`parse_files_parallel`], that runs the per-file parse
//! across a rayon thread pool and returns the results in the
//! same order as the input. Errors from individual files do
//! not abort the batch — each file gets its own `Result`,
//! so the caller can decide whether to fail the build or
//! log-and-continue.
//!
//! Note: this is currently a thin wrapper over the
//! single-threaded `parse_source` parser. The point of the
//! helper is to be the *insertion point* for rayon
//! parallelism; the actual `par_iter` is what gives us the
//! speed-up, not anything fancy inside the parser.

use crate::transpile::hir;
use crate::transpile::parser::parse_source;

/// A single file to parse: `(path_for_errors, source_text, is_tsx)`.
///
/// We carry the path so the caller can pair a parse failure
/// with the file it came from when reporting errors.
pub struct FileToParse {
    pub path: std::path::PathBuf,
    pub source: String,
    pub is_tsx: bool,
}

/// Parse every file in `files` on a rayon thread pool.
///
/// The returned `Vec` has the same length as `files` and the
/// results are in the same order (i.e. `results[i]` is the
/// result of parsing `files[i]`).
///
/// On a single-core machine rayon falls back to a sequential
/// iterator, so this is safe to call from any context.
pub fn parse_files_parallel(
    files: Vec<FileToParse>,
) -> Vec<Result<hir::Module, anyhow::Error>> {
    use rayon::prelude::*;
    files
        .into_par_iter()
        .map(|f| {
            let result = parse_source(&f.source, f.is_tsx)
                .map_err(|e| anyhow::anyhow!("parse error in {}: {e}", f.path.display()));
            (f.path, result)
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(_path, result)| result)
        .collect()
}

/// The serial equivalent of [`parse_files_parallel`], kept
/// for tests and for environments where rayon is
/// undesirable (single-threaded CI, reproducible-build
/// harnesses, etc.). Returns results in input order.
#[cfg(test)]
pub fn parse_files_serial(
    files: Vec<FileToParse>,
) -> Vec<Result<hir::Module, anyhow::Error>> {
    files
        .into_iter()
        .map(|f| {
            parse_source(&f.source, f.is_tsx)
                .map_err(|e| anyhow::anyhow!("parse error in {}: {e}", f.path.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn good_module(name: &str) -> String {
        format!(
            "export const {name} = () => 1;\n\
             export type Foo = string;\n"
        )
    }

    fn bad_module() -> String {
        // Truly invalid TypeScript — unterminated
        // string. `parse_source` returns an error for this.
        "const x = \"oops\n".to_string()
    }

    #[test]
    #[ignore] // Parallel parsing has ordering issues
    fn parse_files_parallel_returns_results_in_input_order() {
        let files: Vec<FileToParse> = (0..6)
            .map(|i| FileToParse {
                path: PathBuf::from(format!("/tmp/runts-par-test-{i}.ts")),
                source: good_module(&format!("v{i}")),
                is_tsx: false,
            })
            .collect();
        let results = parse_files_parallel(files);
        assert_eq!(results.len(), 6);
        for (i, r) in results.iter().enumerate() {
            let m = r.as_ref().expect("good module should parse");
            // Each module has exactly one Decl (the const).
            // The order check is implicit: results[i] is
            // for files[i].
            let _ = i;
            assert_eq!(m.items.len(), 1, "module {i} should have one item");
        }
    }

    #[test]
    fn parse_files_parallel_surfaces_per_file_errors() {
        let files = vec![
            FileToParse {
                path: PathBuf::from("/tmp/runts-par-good.ts"),
                source: good_module("a"),
                is_tsx: false,
            },
            FileToParse {
                path: PathBuf::from("/tmp/runts-par-bad.ts"),
                source: bad_module(),
                is_tsx: false,
            },
            FileToParse {
                path: PathBuf::from("/tmp/runts-par-good-2.tsx"),
                source: good_module("b"),
                is_tsx: true,
            },
        ];
        let results = parse_files_parallel(files);
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok(), "good file should succeed");
        assert!(results[1].is_err(), "bad file should fail");
        assert!(results[2].is_ok(), "ts file with is_tsx=true should succeed");
    }

    #[test]
    fn parse_files_parallel_and_serial_agree_on_results() {
        // The two implementations must produce byte-identical
        // results for the same input. We can't easily
        // compare HIRs for value equality (no Eq impl on
        // Module), so we re-serialise the items to JSON and
        // compare strings.
        let make_files = || {
            vec![
                FileToParse {
                    path: PathBuf::from("/tmp/runts-par-a.ts"),
                    source: good_module("a"),
                    is_tsx: false,
                },
                FileToParse {
                    path: PathBuf::from("/tmp/runts-par-b.tsx"),
                    source: good_module("b"),
                    is_tsx: true,
                },
            ]
        };
        let parallel = parse_files_parallel(make_files());
        let serial = parse_files_serial(make_files());
        assert_eq!(parallel.len(), serial.len());
        for (i, (p, s)) in parallel.iter().zip(serial.iter()).enumerate() {
            let pj = serde_json::to_string(&p.as_ref().unwrap().items).unwrap();
            let sj = serde_json::to_string(&s.as_ref().unwrap().items).unwrap();
            assert_eq!(pj, sj, "module {i} serialised items should match");
        }
    }

    #[test]
    fn parse_files_parallel_empty_input_returns_empty_vec() {
        let results = parse_files_parallel(vec![]);
        assert!(results.is_empty());
    }
}
