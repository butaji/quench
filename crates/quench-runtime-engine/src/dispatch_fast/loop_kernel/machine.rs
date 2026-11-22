//! The scalar pass over any set of recurrences (`Machine`): the nodes that read a value an
//! earlier iteration computed, in body order, as steps over a small register file, then
//! every recurrence's next value at once (`p = q; q = r` reads the old `q`).
//!
//! An operand is a register (the recurrences, the scalars the steps read, the steps'
//! results) or an element of the chunk's row (a column computed before the pass), so a step
//! reads either with one select and one load.
use super::recur::{Code, read_later};
use super::run::CHUNK;
use super::shape::{Arg, Shape};

/// Registers of a machine's file.
const REGISTERS: usize = 64;
/// Recurrences a machine carries.
const MAX_RECURRENCES: usize = 16;
/// The recurrence block of a machine with few recurrences.
const SMALL_BLOCK: usize = 4;

/// The registers `count` recurrences hold.
fn block(count: usize) -> usize {
    if count <= SMALL_BLOCK { SMALL_BLOCK } else { MAX_RECURRENCES }
}

/// An operand naming a register (its low bits) rather than a row element offset.
const REGISTER: u32 = 1 << 31;

#[derive(Clone, Copy)]
struct Step {
    /// The operator and operand forms (`kind`).
    kind: Kind,
    /// The operator, for the general form.
    code: Code,
    /// A register index or a row offset, as the kind says.
    a: u32,
    b: u32,
    register: u8,
    /// The step's column (or the dump column when no later reader needs it).
    out: u32,
}

/// A step's operator and operand forms, one value of a flat match: an arithmetic operator
/// (`ARITH_FORMS` apart) with each operand a register or a row element; any bitwise operator
/// takes the general form, which tests the operands' forms.
#[derive(Clone, Copy)]
struct Kind(u8);

/// Forms per arithmetic operator: whether `a`, then `b`, is a register.
const ARITH_FORMS: u8 = 4;
const GENERAL: u8 = 4 * ARITH_FORMS;

fn kind(code: Code, a: u32, b: u32) -> Kind {
    let forms = u8::from(a & REGISTER != 0) * 2 + u8::from(b & REGISTER != 0);
    let op = match code {
        Code::Add => 0,
        Code::Subtract => 1,
        Code::Multiply => 2,
        Code::Divide => 3,
        _ => return Kind(GENERAL + forms),
    };
    Kind(op * ARITH_FORMS + forms)
}

pub(super) use quench_runtime::vm::engine_types::Machine;

#[quench_macros::ext(MachineOps1)]
impl Machine {
    /// The machine running the scalar pass's nodes but `side`'s.
    pub(super) fn of(shape: &Shape, side: &[u16]) -> Option<Self> {
        let count = shape.phis.len();
        if count > MAX_RECURRENCES {
            return None;
        }
        let dump = u32::try_from(shape.nodes.len() * CHUNK).ok()?;
        // The recurrences hold a fixed block of registers, so an iteration's end copies a
        // block of constant size (`block`).
        let mut next = block(count);
        let mut registers = [u8::MAX; 256];
        let mut scalars: Vec<(u8, u8)> = Vec::new();
        let take = |next: &mut usize| -> Option<u8> {
            *next += 1;
            u8::try_from(*next - 1).ok().filter(|&r| usize::from(r) < REGISTERS)
        };
        let mut operand = |arg: Arg, next: &mut usize, registers: &[u8; 256]| -> Option<u32> {
            Some(match arg {
                Arg::Phi(phi) => REGISTER | u32::from(phi),
                Arg::Col(n) if shape.nodes[usize::from(n)].seq => {
                    REGISTER | u32::from(*registers.get(usize::from(n)).filter(|&&r| r != u8::MAX)?)
                }
                Arg::Col(n) => u32::try_from(usize::from(n) * CHUNK).ok()?,
                Arg::Scalar(slot) => REGISTER | u32::from(match scalars.iter().find(|&&(_, s)| s == slot) {
                    Some(&(register, _)) => register,
                    None => {
                        let register = take(next)?;
                        scalars.push((register, slot));
                        register
                    }
                }),
                Arg::Affine(_) => return None,
            })
        };
        let mut steps = Vec::with_capacity(shape.seq.len());
        for &id in shape.seq.iter().filter(|id| !side.contains(id)) {
            let node = shape.nodes[usize::from(id)];
            let (a, b) = (operand(node.a, &mut next, &registers)?, operand(node.b, &mut next, &registers)?);
            let register = take(&mut next)?;
            *registers.get_mut(usize::from(id))? = register;
            let out = if read_later(shape, side, id) { u32::try_from(usize::from(id) * CHUNK).ok()? } else { dump };
            let code = Code::of(node.op, true);
            steps.push(Step { kind: kind(code, a, b), code, a: a & !REGISTER, b: b & !REGISTER, register, out });
        }
        let finals = shape.phis.iter().map(|&(_, arg)| operand(arg, &mut next, &registers)).collect::<Option<Vec<u32>>>()?;
        Some(Self { steps, scalars, finals })
    }

    /// Whether a step is a bitwise operator.
    pub(super) fn has_bitwise(&self) -> bool {
        self.steps.iter().any(|step| matches!(step.code, Code::Bits { .. }))
    }

    /// Columns past the nodes' this machine uses: the dump.
    pub(super) fn columns(&self) -> usize {
        1
    }

    pub(super) fn run(&self, phis: &mut [f64], scalars: &[f64], scratch: &mut [f64], len: usize) {
        let mut file = [0.0f64; REGISTERS];
        let count = phis.len();
        file[..count].copy_from_slice(phis);
        for &(register, slot) in &self.scalars {
            file[usize::from(register)] = scalars[usize::from(slot)];
        }
        let base = scratch.as_mut_ptr();
        let registers = file.as_mut_ptr();
        let mut next = [0.0f64; MAX_RECURRENCES];
        for k in 0..len {
            // SAFETY: a register index is below `REGISTERS` (`Machine::of`), a row offset is
            // a column's start inside `scratch` (`Recurrence::columns`), and
            // `k < len <= CHUNK`.
            unsafe {
                let row = base.add(k);
                for step in &self.steps {
                    step.run(registers, row);
                }
                for (slot, &operand) in next.iter_mut().zip(&self.finals) {
                    let index = (operand & !REGISTER) as usize;
                    *slot = if operand & REGISTER != 0 { *registers.add(index) } else { *row.add(index) };
                }
                if block(count) == SMALL_BLOCK {
                    std::ptr::copy_nonoverlapping(next.as_ptr(), registers, SMALL_BLOCK);
                } else {
                    std::ptr::copy_nonoverlapping(next.as_ptr(), registers, MAX_RECURRENCES);
                }
            }
        }
        phis.copy_from_slice(&file[..count]);
    }
}

impl Step {
    /// # Safety
    /// As `Machine::run`'s loop.
    #[inline(always)]
    unsafe fn run(&self, registers: *mut f64, row: *mut f64) {
        macro_rules! operand {
            ($register:expr, $index:expr) => {
                // SAFETY: the caller's contract.
                unsafe { if $register { *registers.add($index as usize) } else { *row.add($index as usize) } }
            };
        }
        let (a, b) = (self.a, self.b);
        macro_rules! forms {
            ($($kind:literal => $f:tt, $a:literal, $b:literal;)*) => {
                match self.kind.0 {
                    $($kind => $f(operand!($a, a), operand!($b, b)),)*
                    forms => self.code.apply(operand!(forms & 2 != 0, a), operand!(forms & 1 != 0, b)),
                }
            };
        }
        let add = |x: f64, y: f64| x + y;
        let subtract = |x: f64, y: f64| x - y;
        let multiply = |x: f64, y: f64| x * y;
        let divide = |x: f64, y: f64| x / y;
        let value = forms! {
            0 => add, false, false; 1 => add, false, true; 2 => add, true, false; 3 => add, true, true;
            4 => subtract, false, false; 5 => subtract, false, true; 6 => subtract, true, false; 7 => subtract, true, true;
            8 => multiply, false, false; 9 => multiply, false, true; 10 => multiply, true, false; 11 => multiply, true, true;
            12 => divide, false, false; 13 => divide, false, true; 14 => divide, true, false; 15 => divide, true, true;
        };
        // SAFETY: the caller's contract.
        unsafe {
            *registers.add(usize::from(self.register)) = value;
            *row.add(self.out as usize) = value;
        }
    }
}
