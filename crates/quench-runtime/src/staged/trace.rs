//! A recording stage: the operations a row runs, with every per-site operand a numbered hole.
//! A stencil stage numbers holes by call order, so a row must call `Stage::site` in an order
//! its operand values never change; this stage is the check (the same row over two sites
//! gives one trace, and only the hole values differ).

use super::{Stage, Table};
use super::region::Access;

/// An operand: a fixed constant, or the numbered hole a `site` call made.
#[derive(Clone, Copy)]
pub(crate) struct Imm {
    value: u64,
    hole: Option<u32>,
}

impl From<u64> for Imm { fn from(value: u64) -> Self { Self { value, hole: None } } }
impl From<u32> for Imm { fn from(value: u32) -> Self { Self::from(u64::from(value)) } }

/// The trace of one row run: operations with word numbers, and the value each hole took.
#[derive(Default)]
pub(crate) struct Trace {
    pub(crate) ops: Vec<String>,
    pub(crate) holes: Vec<u64>,
    words: u32,
}

impl Trace {
    fn word(&mut self, op: String) -> u32 {
        self.ops.push(op);
        self.words += 1;
        self.words - 1
    }

    fn operand(imm: Imm) -> String {
        imm.hole.map_or_else(|| format!("{:#x}", imm.value), |hole| format!("hole{hole}"))
    }
}

crate::projection!(Trace => Concrete);

impl Stage for Trace {
    type Word = u32;
    type Proof = ();
    type Cond = u32;
    type Miss = ();
    type Imm = Imm;

    fn site(&mut self, v: u64) -> Imm {
        self.holes.push(v);
        Imm { value: v, hole: Some(self.holes.len() as u32 - 1) }
    }
    fn constant(&mut self, bits: u64) -> u32 { self.word(format!("constant {bits:#x}")) }
    fn load_u32(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_u32 w{base}+{offset}")) }
    fn load_u64(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_u64 w{base}+{offset}")) }
    fn load_u8(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_u8 w{base}+{offset}")) }
    fn load_u16(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_u16 w{base}+{offset}")) }
    fn load_site(&mut self, _at: Access, base: u32, offset: Imm) -> u32 {
        self.word(format!("load_site w{base}+{}", Self::operand(offset)))
    }
    fn load_table(&mut self, _at: Access, address: u32) -> u32 { self.word(format!("load_table w{address}")) }
    fn add(&mut self, a: u32, b: u32) -> u32 { self.word(format!("add w{a} w{b}")) }
    fn or(&mut self, a: u32, b: u32) -> u32 { self.word(format!("or w{a} w{b}")) }
    fn and(&mut self, a: u32, b: u32) -> u32 { self.word(format!("and w{a} w{b}")) }
    fn shr(&mut self, a: u32, b: u32) -> u32 { self.word(format!("shr w{a} w{b}")) }
    fn imm_word(&mut self, imm: Imm) -> u32 { self.word(format!("imm {}", Self::operand(imm))) }
    fn mul_imm(&mut self, a: u32, imm: impl Into<Imm>) -> u32 { self.word(format!("mul_imm w{a} {}", Self::operand(imm.into()))) }
    fn sar32_imm(&mut self, a: u32, shift: u32) -> u32 { self.word(format!("sar32_imm w{a} {shift}")) }
    fn store_gate(&mut self, space: super::StagedRecordSpace, record_id: u32, value: u32) -> u32 { self.word(format!("store_gate {space:?} w{record_id} w{value}")) }
    fn immediate_store_open(&mut self) -> u32 { self.word("immediate_store_open".to_string()) }
    fn assumption(&mut self, watch: Imm) -> u32 { self.word(format!("assumption {}", Self::operand(watch))) }
    fn ult(&mut self, a: u32, b: u32) -> u32 { self.word(format!("ult w{a} w{b}")) }
    fn eq(&mut self, a: u32, b: u32) -> u32 { self.word(format!("eq w{a} w{b}")) }
    /// The body is recorded once, over fresh carried words, and its exits are fresh words too.
    fn loop_words<const N: usize>(&mut self, init: [u32; N], mut body: impl FnMut(&mut Self, [u32; N]) -> (u32, [u32; N]))
        -> [u32; N] {
        let carried = init.map(|word| self.word(format!("loop_carried w{word}")));
        self.ops.push(format!("loop {carried:?}"));
        let (go_on, next) = body(self, carried);
        self.ops.push(format!("loop_back w{go_on} {next:?}"));
        std::array::from_fn(|index| self.word(format!("loop_exit {index} of {carried:?}")))
    }
    fn uextend32(&mut self, a: u32) -> u32 { self.word(format!("uextend32 w{a}")) }
    fn sub(&mut self, a: u32, b: u32) -> u32 { self.word(format!("sub w{a} w{b}")) }
    fn select(&mut self, cond: u32, then: u32, otherwise: u32) -> u32 { self.word(format!("select w{cond} w{then} w{otherwise}")) }
    fn store_u32(&mut self, _at: Access, base: u32, offset: i32, value: u32) { self.ops.push(format!("store_u32 w{base}+{offset} w{value}")); }
    fn store_u64(&mut self, _at: Access, base: u32, offset: i32, value: u32) { self.ops.push(format!("store_u64 w{base}+{offset} w{value}")); }
    fn load_i8(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_i8 w{base}+{offset}")) }
    fn load_i16(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_i16 w{base}+{offset}")) }
    fn load_i32(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_i32 w{base}+{offset}")) }
    fn load_f32(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_f32 w{base}+{offset}")) }
    fn load_f64(&mut self, _at: Access, base: u32, offset: i32) -> u32 { self.word(format!("load_f64 w{base}+{offset}")) }
    fn box_number(&mut self, number: super::Float64<u32>) -> u32 { self.word(format!("box_number w{}", number.word())) }
    fn box_raw_number(&mut self, number: super::Float64<u32>) -> u32 { self.word(format!("box_raw_number w{}", number.word())) }
    fn canonical_float(&mut self, bits: u32) -> u32 { self.word(format!("canonical_float w{bits}")) }
    fn raw_number(&mut self, bits: u32) -> u32 { self.word(format!("raw_number w{bits}")) }
    fn store_u8(&mut self, _at: Access, base: u32, offset: i32, value: u32) { self.ops.push(format!("store_u8 w{base}+{offset} w{value}")); }
    fn store_u16(&mut self, _at: Access, base: u32, offset: i32, value: u32) { self.ops.push(format!("store_u16 w{base}+{offset} w{value}")); }
    fn store_f32(&mut self, _at: Access, base: u32, offset: i32, value: u32) { self.ops.push(format!("store_f32 w{base}+{offset} w{value}")); }
    fn store_f64(&mut self, _at: Access, base: u32, offset: i32, value: u32) { self.ops.push(format!("store_f64 w{base}+{offset} w{value}")); }
    fn f32_to_f64(&mut self, a: u32) -> u32 { self.word(format!("f32_to_f64 w{a}")) }
    fn f64_to_f32(&mut self, a: u32) -> u32 { self.word(format!("f64_to_f32 w{a}")) }
    fn clamp_u8(&mut self, a: u32) -> u32 { self.word(format!("clamp_u8 w{a}")) }
    fn not(&mut self, cond: u32) -> u32 { self.word(format!("not w{cond}")) }
    fn any(&mut self, a: u32, b: u32) -> u32 { self.word(format!("any w{a} w{b}")) }
    fn pick(&mut self, cond: u32, then: u32, otherwise: u32) -> u32 { self.word(format!("pick w{cond} w{then} w{otherwise}")) }
    fn number_truthy(&mut self, bits: u32) -> u32 { self.word(format!("number_truthy w{bits}")) }
    fn shape_in(&mut self, shape: u32, shapes: &[u32]) -> u32 { self.word(format!("shape_in w{shape} {shapes:?}")) }
    fn shape_in_site(&mut self, shape: u32, shapes: &[u32]) -> u32 {
        let mut found = None;
        for &candidate in shapes {
            let candidate = self.site(u64::from(candidate));
            let same = self.eq_imm(shape, candidate);
            found = Some(found.map_or(same, |any| self.any(any, same)));
        }
        found.unwrap_or_else(|| self.eq_imm(shape, u64::MAX))
    }
    fn choose(&mut self, cond: u32, then: impl FnOnce(&mut Self) -> u32, otherwise: impl FnOnce(&mut Self) -> u32) -> u32 {
        self.ops.push(format!("choose w{cond}"));
        let (a, b) = (then(self), otherwise(self));
        self.word(format!("join w{a} w{b}"))
    }
    fn guard(&mut self, cond: u32, _miss: ()) -> Option<()> {
        self.ops.push(format!("guard w{cond}"));
        Some(())
    }
    fn known(&self, _cond: u32) -> Option<bool> { None }
    fn table(&mut self, table: Table) -> u32 { self.word(format!("table {table:?}")) }
}
