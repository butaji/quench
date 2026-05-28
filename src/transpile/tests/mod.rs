//! Comprehensive tests for the transpilation pipeline

#[cfg(test)]
mod parser;

#[cfg(test)]
mod codegen;

#[cfg(test)]
mod analyzer;

#[cfg(test)]
mod routegen;

#[cfg(test)]
mod integration;

#[cfg(test)]
use parser::parser_tests;
#[cfg(test)]
use codegen::codegen_tests;
#[cfg(test)]
use analyzer::analyzer_tests;
#[cfg(test)]
use routegen::routegen_tests;
#[cfg(test)]
use integration::integration_tests;
