//! Comprehensive spec tests for Expressions & Operators (SUPPORTED_SUBSET.md 2.3)
//!
//! Verifies: parser produces correct HIR, codegen produces meaningful Rust, output patterns match.

#[cfg(test)]
mod arithmetic;

#[cfg(test)]
mod comparison;

#[cfg(test)]
mod logical;

#[cfg(test)]
mod bitwise;

#[cfg(test)]
mod unary;

#[cfg(test)]
mod ternary_templates;

#[cfg(test)]
mod member_call;

#[cfg(test)]
mod literals;

#[cfg(test)]
mod complex;
