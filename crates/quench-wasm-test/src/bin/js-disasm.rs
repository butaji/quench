//! Print the residual bytecode a JavaScript script compiles to, for reading what the
//! interpreter and JIT tiers run.

use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: js-disasm <script.js>");
        return ExitCode::from(2);
    };
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    match quench_runtime::Engine::compile(quench_runtime::ExecutionRequest::script(&source, &path)) {
        Ok(program) => {
            print!("{}", program.disassemble());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error:?}");
            ExitCode::FAILURE
        }
    }
}
