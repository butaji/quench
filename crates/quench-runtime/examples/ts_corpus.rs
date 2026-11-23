//! Compile every TypeScript file named on stdin as a module; print one verdict per file:
//! `ok <path>` or `err <path>\t<first diagnostic>`.
use std::io::BufRead;

fn main() {
    for line in std::io::stdin().lock().lines() {
        let path = line.expect("path line");
        let Ok(source) = std::fs::read_to_string(&path) else { continue };
        let verdict = std::panic::catch_unwind(|| quench_runtime::Engine::specialize_typescript_module(&source, &path));
        match verdict {
            Ok(Ok(_)) => println!("ok {path}"),
            Ok(Err(diagnostics)) => {
                let first = diagnostics.first().map(|d| format!("{d:?}")).unwrap_or_default();
                println!("err {path}\t{first}");
            }
            Err(_) => println!("panic {path}"),
        }
    }
}

// Links the runtime layer crates (quench-layers).
extern crate quench_layers;
