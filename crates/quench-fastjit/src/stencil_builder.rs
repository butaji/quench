//! The FastJIT back-end of [`RowBuilder`]: operands are holes, successors are relocated
//! `return_call`s, and every dynamic value lives in the frame or a pinned register.
use super::row::{Effects, MemKind, Op, Operand, Region, RowBuilder, Source, Ty};
use super::{HOLE_VALUE, NEXT, TAKEN, EXIT, successor};
use cranelift_codegen::ir::{self, ExternalName, GlobalValueData, InstBuilder, MemFlagsData, UserExternalName, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;

/// The values a stencil passes on in registers: raw int32s in the value registers, doubles in
/// the xmm registers.
#[derive(Clone, Copy)]
pub struct Pins {
    pub ints: [ir::Value; super::PINNED],
    pub floats: [ir::Value; super::PINNED_F],
}

pub struct StencilBuilder<'a, 'f> {
    pub b: &'a mut FunctionBuilder<'f>,
    pub frame: ir::Value,
    pub context: ir::Value,
    /// The pinned registers, passed to every successor.
    pub pins: Pins,
    pub(crate) next: ir::FuncRef,
    pub(crate) taken: ir::FuncRef,
    pub(crate) exits: Vec<Option<ir::FuncRef>>,
    pub(crate) pending: Vec<(ir::Block, ir::FuncRef, Vec<ir::Value>)>,
}

fn clif(ty: Ty) -> ir::Type {
    match ty {
        Ty::I32 => types::I32,
        Ty::I64 | Ty::Word => types::I64,
        Ty::F64 => types::F64,
    }
}

impl StencilBuilder<'_, '_> {
    /// The arguments of every successor: frame, context and the pinned values.
    pub(crate) fn args(&self) -> Vec<ir::Value> {
        [self.frame, self.context].into_iter().chain(self.pins.ints).chain(self.pins.floats).collect()
    }

    fn immediate(&mut self, operand: Operand) -> ir::Value {
        match operand {
            Operand::Const(value) => self.b.ins().iconst(types::I64, value),
            Operand::Hole(index) => {
                let name = self.b.func.declare_imported_user_function(UserExternalName::new(HOLE_VALUE, index));
                let symbol = self.b.func.create_global_value(GlobalValueData::Symbol {
                    name: ExternalName::User(name),
                    offset: 0.into(),
                    colocated: false,
                    tls: false,
                });
                self.b.ins().symbol_value(types::I64, symbol)
            }
        }
    }

    fn address(&mut self, src: Source<ir::Value>) -> ir::Value {
        match src {
            Source::Frame(offset) => {
                let offset = self.immediate(offset);
                self.b.ins().iadd(self.frame, offset)
            }
            Source::Pinned(0) => self.frame,
            Source::Pinned(_) => self.context,
            Source::Value(v) => v,
        }
    }

    /// A boolean (`icmp`, overflow flag) is logically inverted; an integer bitwise.
    fn invert(&mut self, x: ir::Value) -> ir::Value {
        if self.b.func.dfg.value_type(x) == types::I8 {
            self.b.ins().bxor_imm_u(x, 1)
        } else {
            self.b.ins().bnot(x)
        }
    }

    fn arithmetic(&mut self, o: Op, x: ir::Value, y: ir::Value) -> ir::Value {
        match o {
            Op::Add => self.b.ins().iadd(x, y),
            Op::Sub => self.b.ins().isub(x, y),
            Op::And => self.b.ins().band(x, y),
            Op::Or => self.b.ins().bor(x, y),
            _ => self.b.ins().bxor(x, y),
        }
    }

    /// A cold block that leaves through exit hole `exit` with the pinned values as they are now,
    /// for a helper that takes a failure block. It is filled and sealed once the row is emitted
    /// (`finish`).
    pub fn exit_block(&mut self, exit: u32) -> ir::Block {
        let target = *self.exits.resize_and_get(exit as usize).get_or_insert_with(|| successor(self.b, EXIT + exit));
        let side = self.b.create_block();
        self.b.set_cold_block(side);
        self.pending.push((side, target, self.args()));
        side
    }

    /// Fills the blocks `exit_block` made; the row's own code is complete.
    pub(crate) fn finish(&mut self) {
        for (side, target, args) in std::mem::take(&mut self.pending) {
            self.b.switch_to_block(side);
            self.b.ins().return_call(target, &args);
            self.b.seal_block(side);
        }
    }

    /// Leave through exit hole `exit` when `cond` equals `when`, else continue. The branch tests
    /// `cond` itself, so a comparison or an overflow flag feeding it stays one compare and jump.
    pub(crate) fn leave_when(&mut self, cond: ir::Value, when: bool, exit: u32) {
        let target = *self.exits.resize_and_get(exit as usize).get_or_insert_with(|| successor(self.b, EXIT + exit));
        let args = self.args();
        let (side, rest) = (self.b.create_block(), self.b.create_block());
        if when {
            self.b.ins().brif(cond, side, &[], rest, &[]);
        } else {
            self.b.ins().brif(cond, rest, &[], side, &[]);
        }
        self.b.switch_to_block(side);
        self.b.ins().return_call(target, &args);
        self.b.seal_block(side);
        self.b.switch_to_block(rest);
        self.b.seal_block(rest);
    }
}

impl RowBuilder for StencilBuilder<'_, '_> {
    type V = ir::Value;
    /// An exit kind: guard `kind` reaches exit hole `EXIT + kind`.
    type Exit = u32;

    fn read(&mut self, src: Source<ir::Value>, ty: Ty) -> ir::Value {
        match src {
            Source::Value(v) => v,
            Source::Pinned(0) => self.frame,
            Source::Pinned(_) => self.context,
            Source::Frame(_) => {
                let address = self.address(src);
                self.b.ins().load(clif(ty), MemFlagsData::trusted(), address, 0)
            }
        }
    }

    fn write(&mut self, dst: Source<ir::Value>, v: ir::Value) {
        let address = self.address(dst);
        self.b.ins().store(MemFlagsData::trusted(), v, address, 0);
    }

    fn operand(&mut self, op: Operand, _ty: Ty) -> ir::Value {
        self.immediate(op)
    }

    fn op(&mut self, o: Op, _ty: Ty, args: &[ir::Value], _eff: Effects) -> ir::Value {
        let (x, y) = (args[0], args.get(1).copied().unwrap_or(args[0]));
        match o {
            Op::Eq => self.b.ins().icmp(IntCC::Equal, x, y),
            Op::Ne => self.b.ins().icmp(IntCC::NotEqual, x, y),
            Op::SignedLess => self.b.ins().icmp(IntCC::SignedLessThan, x, y),
            Op::Overflow => self.b.ins().sadd_overflow(x, y).1,
            Op::Not => self.invert(x),
            Op::Add | Op::Sub | Op::And | Op::Or | Op::Xor => self.arithmetic(o, x, y),
        }
    }

    fn guard(&mut self, cond: ir::Value, exit: u32) {
        // `guard` continues when `cond` holds: leave when it does not.
        self.leave_when(cond, false, exit);
    }

    fn load(&mut self, _region: Region, base: ir::Value, off: Operand, ty: Ty, kind: MemKind) -> ir::Value {
        let offset = self.immediate(off);
        let address = self.b.ins().iadd(base, offset);
        let flags = match kind {
            MemKind::Plain => MemFlagsData::trusted(),
            MemKind::Invariant => MemFlagsData::trusted().with_readonly().with_can_move(),
        };
        self.b.ins().load(clif(ty), flags, address, 0)
    }

    fn store(&mut self, _region: Region, base: ir::Value, off: Operand, v: ir::Value) {
        let offset = self.immediate(off);
        let address = self.b.ins().iadd(base, offset);
        self.b.ins().store(MemFlagsData::trusted(), v, address, 0);
    }

    fn cold<F: FnOnce(&mut Self) -> ir::Value>(&mut self, f: F) -> ir::Value {
        // A cold block that rejoins with a value through a block parameter.
        let (side, join) = (self.b.create_block(), self.b.create_block());
        self.b.set_cold_block(side);
        let result = self.b.append_block_param(join, types::I64);
        self.b.ins().jump(side, &[]);
        self.b.switch_to_block(side);
        self.b.seal_block(side);
        let value = f(self);
        self.b.ins().jump(join, &[value.into()]);
        self.b.switch_to_block(join);
        self.b.seal_block(join);
        result
    }

    fn if_eq_chain<F: FnOnce(&mut Self) -> ir::Value>(
        &mut self,
        key: ir::Value,
        arms: &[(Operand, &dyn Fn(&mut Self) -> ir::Value)],
        miss: F,
    ) -> ir::Value {
        let join = self.b.create_block();
        let result = self.b.append_block_param(join, types::I64);
        for (case, arm) in arms {
            let want = self.immediate(*case);
            let hit = self.b.ins().icmp(IntCC::Equal, key, want);
            let (taken, rest) = (self.b.create_block(), self.b.create_block());
            self.b.ins().brif(hit, taken, &[], rest, &[]);
            self.b.switch_to_block(taken);
            self.b.seal_block(taken);
            let value = arm(self);
            self.b.ins().jump(join, &[value.into()]);
            self.b.switch_to_block(rest);
            self.b.seal_block(rest);
        }
        let value = miss(self);
        self.b.ins().jump(join, &[value.into()]);
        self.b.switch_to_block(join);
        self.b.seal_block(join);
        result
    }

    fn next(&mut self) {
        let args = self.args();
        self.b.ins().return_call(self.next, &args);
    }

    fn branch(&mut self, cond: ir::Value) {
        // The continuation is laid out last, so the row's `jmp next` ends the stencil and
        // adjacency drops it.
        let (args, taken) = (self.args(), self.taken);
        let (side, rest) = (self.b.create_block(), self.b.create_block());
        self.b.ins().brif(cond, side, &[], rest, &[]);
        self.b.switch_to_block(side);
        self.b.ins().return_call(taken, &args);
        self.b.seal_block(side);
        self.b.set_cold_block(rest);
        self.b.switch_to_block(rest);
        self.b.seal_block(rest);
    }
}

/// `Vec<Option<_>>` growth helper for the exit cache.
trait ResizeGet<T> {
    fn resize_and_get(&mut self, index: usize) -> &mut Option<T>;
}
impl<T: Clone> ResizeGet<T> for Vec<Option<T>> {
    fn resize_and_get(&mut self, index: usize) -> &mut Option<T> {
        if self.len() <= index {
            self.resize(index + 1, None);
        }
        &mut self[index]
    }
}

/// The successor holes of a fresh stencil body.
pub(crate) fn new_successors(b: &mut FunctionBuilder) -> (ir::FuncRef, ir::FuncRef) {
    (successor(b, NEXT), successor(b, TAKEN))
}
