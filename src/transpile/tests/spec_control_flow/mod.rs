//! Spec control flow tests — section 2.4 of SUPPORTED_SUBSET.md
//!
//! Covers: conditional, switch, loops, jump statements, exception handling

#[cfg(test)]
mod helpers;

#[cfg(test)]
mod conditional;

#[cfg(test)]
mod switch;

#[cfg(test)]
mod loops;

#[cfg(test)]
mod jumps;

#[cfg(test)]
mod exceptions;

#[cfg(test)]
mod edge_cases;
