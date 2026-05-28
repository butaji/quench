//! Type parsing utilities - minimal implementation

use super::super::hir::*;
use super::Parser;
use anyhow::Result;

pub struct TypeParser<'a> { parser: &'a mut Parser }

impl<'a> TypeParser<'a> {
    pub fn new(parser: &'a mut Parser) -> Self { Self { parser } }
    pub fn parse_type(&mut self) -> Result<Type> { Ok(Type::Unknown) }
}
