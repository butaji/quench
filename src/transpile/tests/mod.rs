//! Comprehensive tests for the transpilation pipeline

#[cfg(test)]
pub mod analyzer;

#[cfg(test)]
pub mod completeness_codegen;

#[cfg(test)]
pub mod completeness_parser;

#[cfg(test)]
pub mod integration;

#[cfg(test)]
pub mod parser;

#[cfg(test)]
pub mod routegen;

#[cfg(test)]
pub mod spec_async_runtime;

// Temporarily disabled — helper visibility issues need fixing
// #[cfg(test)]
// pub mod spec_control_flow;

#[cfg(test)]
pub mod rq_parity;

// #[cfg(test)]
// pub mod spec_data_structures;

#[cfg(test)]
pub mod spec_modules;

// #[cfg(test)]
// pub mod spec_vars_functions;

#[cfg(test)]
pub mod spec_roundtrip;

// #[cfg(test)]
// pub mod spec_jsx;

#[cfg(test)]
pub mod spec_classes;

#[cfg(test)]
pub mod spec_stdlib;
