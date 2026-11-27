//! The wide-integer lowering of the runtime's `Lowering` (`wasm.rs`). Its emission names the
//! runtime's `Lowering` and `Diagnostic`, so the impl stays in the runtime with `wasm/integer.rs`.

#![allow(unused_imports)]
use super::*;

impl crate::wasm::Lowering<'_> {
    /// Wide results are two ordinary stack values, not a separate numeric payload.
    pub fn wide_integer_operator(
        &mut self,
        op: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        use crate::staged::wasm_emit;
        use I64BinaryOperator as Binary;
        let (arithmetic, product_high) = match op {
            wasmparser::Operator::I64Add128 => (Binary::Add, None),
            wasmparser::Operator::I64Sub128 => (Binary::Subtract, None),
            wasmparser::Operator::I64MulWideS => {
                (Binary::Multiply, Some(Binary::MultiplyHighSigned))
            }
            wasmparser::Operator::I64MulWideU => {
                (Binary::Multiply, Some(Binary::MultiplyHighUnsigned))
            }
            _ => return Ok(false),
        };
        if self.path == crate::wasm::Reachability::Dead {
            return Ok(true);
        }
        if let Some(product_high) = product_high {
            let right = self.pop()?;
            let left = self.pop()?;
            let depth = self.depth;
            self.depth = right + 1;
            let high = self.push()?;
            self.emit_emission(wasm_emit::i64_binary(product_high, high, left, right))?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, left, left, right))?;
            self.emit_emission(wasm_emit::move_register(right, high))?;
            self.depth = depth;
        } else {
            let right_high = self.pop()?;
            let right_low = self.pop()?;
            let left_high = self.pop()?;
            let left_low = self.pop()?;
            let depth = self.depth;
            self.depth = right_high + 1;
            let low = self.push()?;
            let carry = self.push()?;
            let high = self.push()?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, low, left_low, right_low))?;
            let (compare_left, compare_right) = if arithmetic == Binary::Add {
                (low, left_low)
            } else {
                (left_low, right_low)
            };
            self.emit_emission(wasm_emit::i64_binary(
                Binary::LessUnsigned,
                carry,
                compare_left,
                compare_right,
            ))?;
            self.emit_emission(wasm_emit::scalar_convert(
                carry,
                carry,
                crate::wasm::conversion::ScalarConversionOperator::ExtendI32Unsigned,
            ))?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, high, left_high, right_high))?;
            self.emit_emission(wasm_emit::i64_binary(arithmetic, high, high, carry))?;
            self.emit_emission(wasm_emit::move_register(left_low, low))?;
            self.emit_emission(wasm_emit::move_register(left_high, high))?;
            self.depth = depth;
        }
        self.push()?;
        self.push()?;
        Ok(true)
    }
}
