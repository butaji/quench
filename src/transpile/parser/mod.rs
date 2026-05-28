//! TypeScript/TSX parser - minimal implementation

pub mod lexer;
pub mod types;

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::fs;
use super::hir::*;

pub struct Parser { source: String, pos: usize }

impl Parser {
    pub fn new() -> Self { Self { source: String::new(), pos: 0 } }
    pub fn parse_source(&mut self, source: &str) -> Result<Module> { self.source = source.to_string(); self.pos = 0; self.parse_module() }
    pub fn parse_file(&mut self, path: &PathBuf) -> Result<Module> { let s = fs::read_to_string(path).context("Failed to read file")?; self.parse_source(&s) }

    fn parse_module(&mut self) -> Result<Module> { Ok(Module { source: String::new(), items: vec![], types: std::collections::HashMap::new() }) }
}

impl Default for Parser { fn default() -> Self { Self::new() } }
