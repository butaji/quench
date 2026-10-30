use quench_runtime::{ExecutionMode, ExecutionRequest, ResidualProgram, Runtime, SystemHost};

fn main() {
    assert!(quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY);
    let program = ResidualProgram::read_binary(std::path::Path::new(
        &std::env::args().nth(1).expect("archived program path"),
    )).unwrap();
    for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled, ExecutionMode::NativeOnly] {
        let mut runtime = Runtime::new(SystemHost);
        runtime.set_execution_mode(mode);
        let diagnostics = runtime.compile_entry(ExecutionRequest::script(
            "let compilationOnly = ;", "unavailable.js",
        )).unwrap_err();
        assert!(diagnostics[0].to_string().contains("entry source compilation is unavailable"));
        let error = runtime.execute(&program).unwrap_err();
        assert!(error.to_string().contains("native guest code required"), "{mode:?}: {error}");
    }
    println!("execution-only support rejects source and unlinked code in every mode");
}
