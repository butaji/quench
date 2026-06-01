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
pub mod spec_expressions;
