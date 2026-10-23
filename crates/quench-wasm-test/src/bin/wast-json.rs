//! Writes each `.wast` script as JSON commands with binary modules (`wast_to_json`), for
//! running the spec testsuite on engines that take Wasm through a JavaScript host.
//!
//! ```sh
//! wast-json OUT_DIR WAST_FILE...
//! ```
//!
//! `OUT_DIR/<file stem path>.json` mirrors each input's path below the testsuite root.

use std::{env, fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1).map(PathBuf::from);
    let Some(out) = arguments.next() else {
        eprintln!("usage: wast-json OUT_DIR WAST_FILE...");
        return ExitCode::from(2);
    };
    let root = quench_wasm_test::testsuite_root().canonicalize().unwrap_or_default();
    let mut failed = false;
    for file in arguments {
        let absolute = file.canonicalize().unwrap_or_else(|_| file.clone());
        let relative = absolute.strip_prefix(&root).unwrap_or(&file).with_extension("json");
        let target = out.join(relative);
        let result = fs::read_to_string(&file)
            .map_err(|error| error.to_string())
            .and_then(|source| quench_wasm_test::wast_to_json(&file.to_string_lossy(), &source))
            .and_then(|json| {
                fs::create_dir_all(target.parent().expect("output has a parent"))
                    .and_then(|()| fs::write(&target, json.to_string()))
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = result {
            eprintln!("{}: {error}", file.display());
            failed = true;
        }
    }
    if failed { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
