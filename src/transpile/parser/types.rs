//! Type parsing utilities - minimal implementation

use super::super::hir::*;
use super::TsParser;
use anyhow::Result;

pub struct TypeParser<'a> { parser: &'a mut TsParser }

impl<'a> TypeParser<'a> {
    pub fn new(parser: &'a mut TsParser) -> Self { Self { parser } }
    pub fn parse_type(&mut self) -> Result<Type> { Ok(Type::Unknown) }
}
