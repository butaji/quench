//! Module support stubs.

#![allow(dead_code)]

use crate::value::Value;
use std::rc::Rc;

/// Get or create a module namespace for the given specifier.
pub fn get_or_create_module(_specifier: &str) -> Rc<Vec<(String, Value)>> {
    Rc::new(Vec::new())
}

/// Import a named export from a module.
pub fn import_named(_specifier: &str, _name: &str) -> Option<Value> {
    None
}

/// Export a value to a module.
pub fn export_value(_specifier: &str, _name: String, _value: Value) {}
