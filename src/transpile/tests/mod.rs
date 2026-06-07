//! Comprehensive tests for the transpilation pipeline

#[cfg(test)]
pub mod analyzer;

#[cfg(test)]
pub mod completeness_codegen; // Ignored: codegen completeness issues

#[cfg(test)]
pub mod completeness_parser;

#[cfg(test)]
pub mod integration;

#[cfg(test)]
pub mod parser; // Ignored: parser tests have known issues

#[cfg(test)]
pub mod spec_async_runtime; // Ignored: async patterns not fully implemented

#[cfg(test)]
pub mod spec_control_flow; // Ignored: control flow patterns not fully implemented

#[cfg(test)]
pub mod rq_parity;

#[cfg(test)]
pub mod spec_data_structures; // Ignored: data structure handling not fully implemented

#[cfg(test)]
pub mod spec_modules; // Ignored: module handling not fully implemented

#[cfg(test)]
pub mod spec_vars_functions; // Ignored: variable and function handling not fully implemented

#[cfg(test)]
pub mod spec_roundtrip; // Ignored: roundtrip tests have known issues

#[cfg(test)]
pub mod spec_jsx; // Ignored: JSX parsing not implemented

#[cfg(test)]
pub mod spec_classes; // Ignored: class support not fully implemented

#[cfg(test)]
pub mod spec_stdlib; // Ignored: stdlib tests have known issues

#[cfg(test)]
pub mod spec_generators;

#[cfg(test)]
pub mod spec_expressions;

#[cfg(test)]
pub mod spec_types;
