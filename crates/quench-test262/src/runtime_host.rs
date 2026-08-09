//! Adapter from the runner contract to the residual runtime.

use quench_runtime::{
    execute,
    reduce::{reduce_module_source, reduce_source},
};

use crate::Test262Host;

#[derive(Debug, Default)]
pub struct RuntimeHost;

impl Test262Host for RuntimeHost {
    fn run_script(&mut self, source: &str) -> Result<(), String> {
        run_source(source)
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        run_module_source(source)
    }
}

fn run_source(source: &str) -> Result<(), String> {
    let program = reduce_source(source).map_err(|errors| errors.join("; "))?;
    execute::execute(&program.ops)
        .map(|_| ())
        .map_err(|error| format!("residual VM error: {error:?}"))
}

fn run_module_source(source: &str) -> Result<(), String> {
    let program = reduce_module_source(source).map_err(|errors| errors.join("; "))?;
    execute::execute(&program.ops)
        .map(|_| ())
        .map_err(|error| format!("residual VM error: {error:?}"))
}
