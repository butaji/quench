fn main() -> std::process::ExitCode {
    match quench_node::shared_run::run_shared_cli(std::env::args().skip(1)) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("quench-node: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
