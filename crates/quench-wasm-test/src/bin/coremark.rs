//! Run the wasm-coremark-rs module on the shared VM with the same host contract
//! as its comparator harness: import `env.clock_ms: () -> i32`, export `run: () -> f32`.

use std::{env, fs, process::ExitCode, sync::OnceLock, thread, time::Instant};

use quench_runtime::{
    Host, Runtime, WasmHostFunctionId, WasmHostValue, WasmSignature, WasmType, WasmValue,
};
use wasmparser::{ExternalKind, Parser, Payload};

const CLOCK_IMPORT: (&str, &str) = ("env", "clock_ms");
const CLOCK_FUNCTION: WasmHostFunctionId = WasmHostFunctionId(0);
const ENTRY_EXPORT: &str = "run";

/// Milliseconds since first use, truncated to u32 exactly like the comparator harness.
///
/// With `COREMARK_CLOCK_STEPS_MS=a,b,...` each read instead advances a virtual
/// clock by the next listed step, repeating the last one. A schedule fixes
/// how far CoreMark's calibration grows, so the run does a fixed amount of
/// work whose instruction count (for example under cachegrind) compares
/// builds without timing noise. The reported score is then meaningless.
fn clock_ms() -> u32 {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    static SCHEDULE: OnceLock<Option<Vec<u32>>> = OnceLock::new();
    static READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static NOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let schedule = SCHEDULE.get_or_init(|| {
        let steps = env::var("COREMARK_CLOCK_STEPS_MS").ok()?;
        steps
            .split(',')
            .map(|step| step.trim().parse().ok())
            .collect()
    });
    match schedule {
        Some(steps) if !steps.is_empty() => {
            let read = READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let step = steps[read.min(steps.len() - 1)];
            NOW.fetch_add(step, std::sync::atomic::Ordering::Relaxed)
        }
        _ => STARTED.get_or_init(Instant::now).elapsed().as_millis() as u32,
    }
}

struct CoremarkHost;
impl Host for CoremarkHost {
    fn write_line(&mut self, text: &str) {
        println!("{text}");
    }
    fn clock_millis(&mut self) -> f64 {
        f64::from(clock_ms())
    }
    fn call_wasm(
        &mut self,
        id: WasmHostFunctionId,
        _args: &[WasmHostValue],
    ) -> Result<Vec<WasmHostValue>, String> {
        if id != CLOCK_FUNCTION {
            return Err("unknown coremark host function".into());
        }
        Ok(vec![WasmHostValue::I32(clock_ms() as i32)])
    }
}

fn export_index(bytes: &[u8], name: &str) -> Result<u32, String> {
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::ExportSection(exports) = payload.map_err(|error| error.to_string())? {
            for export in exports {
                let export = export.map_err(|error| error.to_string())?;
                if export.name == name && export.kind == ExternalKind::Func {
                    return Ok(export.index);
                }
            }
        }
    }
    Err(format!("missing function export `{name}`"))
}

fn run(path: &str) -> Result<f32, String> {
    let bytes = fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    let entry = export_index(&bytes, ENTRY_EXPORT)?;
    let module = quench_wasm::Engine::new()
        .compile(&bytes)
        .map_err(|error| error.to_string())?
        .lower_shared_module()
        .map_err(|error| error.to_string())?;
    let mut runtime = Runtime::new(CoremarkHost);
    let mut imports = Vec::new();
    for import in module.imports() {
        if (import.module.as_str(), import.name.as_str()) != CLOCK_IMPORT {
            return Err(format!(
                "unsupported import {}.{}",
                import.module, import.name
            ));
        }
        let signature = WasmSignature {
            params: vec![],
            results: vec![WasmType::I32],
        };
        imports.push(
            runtime
                .wasm_host_function(CLOCK_IMPORT.1, CLOCK_FUNCTION, signature)
                .map_err(|error| error.to_string())?,
        );
    }
    let instance = runtime
        .instantiate_wasm_module_with_imports(&module, &imports)
        .map_err(|error| error.to_string())?;
    match runtime.invoke_wasm(&instance, entry, &[]) {
        Ok(Some(WasmValue::F32(bits))) => Ok(f32::from_bits(bits)),
        Ok(other) => Err(format!("`{ENTRY_EXPORT}` returned {other:?}, expected f32")),
        Err(error) => Err(error.to_string()),
    }
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: coremark <coremark-minimal.wasm>");
        return ExitCode::from(2);
    };
    let outcome = thread::Builder::new()
        .stack_size(quench_stack::WORKER_STACK_SIZE)
        .spawn(move || run(&path))
        .expect("reserve coremark worker stack")
        .join()
        .unwrap_or_else(|_| Err("coremark worker panicked".into()));
    match outcome {
        Ok(score) => {
            println!("Score: {score}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
