//! The FastJIT back-end of [`RowBuilder`]: operands are holes, successors are relocated
//! `return_call`s, and every dynamic value lives in the frame or a pinned register.
use super::row::{Effects, MemKind, Op, Operand, Region, RowBuilder, Source, Ty};
use super::{HOLE_VALUE, NEXT, TAKEN, EXIT, successor};
use cranelift_codegen::ir::{self, ExternalName, GlobalValueData, InstBuilder, MemFlagsData, UserExternalName, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;

pub struct StencilBuilder<'a, 'f> {
    pub(crate) b: &'a mut FunctionBuilder<'f>,
    pub(crate) frame: ir::Value,
    pub(crate) context: ir::Value,
    pub(crate) next: ir::FuncRef,
    pub(crate) taken: ir::FuncRef,
    pub(crate) exits: Vec<Option<ir::FuncRef>>,
}

fn clif(ty: Ty) -> ir::Type {
    match ty {
        Ty::I32 => types::I32,
        Ty::I64 | Ty::Word => types::I64,
        Ty::F64 => types::F64,
    }
}

impl StencilBuilder<'_, '_> {
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

    /// Branch on `cond`; `leave` lowers the block taken when it is nonzero.
    fn split(&mut self, cond: ir::Value, cold: bool, leave: impl FnOnce(&mut Self)) {
        let (side, rest) = (self.b.create_block(), self.b.create_block());
        if cold {
            self.b.set_cold_block(side);
        }
        self.b.ins().brif(cond, side, &[], rest, &[]);
        self.b.switch_to_block(side);
        leave(self);
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
        let failed = self.b.ins().bxor_imm_u(cond, 1);
        let target = *self.exits.resize_and_get(exit as usize).get_or_insert_with(|| successor(self.b, EXIT + exit));
        let (frame, context) = (self.frame, self.context);
        self.split(failed, true, |s| {
            s.b.ins().return_call(target, &[frame, context]);
        });
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
        self.b.ins().return_call(self.next, &[self.frame, self.context]);
    }

    fn branch(&mut self, cond: ir::Value) {
        let (frame, context, taken) = (self.frame, self.context, self.taken);
        self.split(cond, false, |s| {
            s.b.ins().return_call(taken, &[frame, context]);
        });
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
