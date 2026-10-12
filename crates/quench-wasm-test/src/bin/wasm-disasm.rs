//! Print the shared-VM residual bytecode lowered from a Wasm binary or text module.

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: wasm-disasm <module.wasm|module.wat>");
        return ExitCode::from(2);
    };
    let engine = quench_wasm::Engine::new();
    let module = if path.ends_with(".wat") {
        fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                engine
                    .compile_wat_with_features(&text, wasmparser::WasmFeatures::all())
                    .map_err(|error| error.to_string())
            })
    } else {
        fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| engine.compile(&bytes).map_err(|error| error.to_string()))
    };
    match module.and_then(|module| module.lower_shared_module().map_err(|e| e.to_string())) {
        Ok(module) => {
            print!("{}", module.residual().disassemble());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
