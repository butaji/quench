//! Loop-invariant constant hoisting: a `LoadConst` inside a loop reloads the same constant on every
//! iteration. The pass loads each such constant once into a register of its own before the loop
//! and has the loop's readers take it from there, so the in-loop load is dropped.
//!
//! A load `p` of constant `K` into `r` is hoisted when `Walk` finds every read of `r` that `p`
//! supplies and no join point or handler carries `r` on to a reader `p` does not reach. Those
//! readers read the hoisted register instead, and `p` becomes a `Nop` that `remove_nops` drops.
//! The hoisted register holds `K` from its loop's preheader on, and only preheaders write it, each
//! with that same value.
//!
//! A loop is used only when no jump from outside it reaches one of its pcs and no handler or return
//! target lies in it. Then every path into the loop runs the preheader placed before its head: the
//! fallthrough from the head's predecessor runs it, while back edges target the head's relocated pc
//! past it and do not.
use super::*;
use crate::bitset::BitSet;
use crate::bytecode::{Handler, Operand, Register, REGISTER_MASK};
use std::collections::BTreeMap;

const LOAD_CONST: Op = the_op(Traits::CONSTANT_LOAD);
/// The most distinct constants one function hoists. Each takes one register of its frame.
const MAX_HOISTED_CONSTANTS: usize = 16;
/// The most instructions one reader walk visits, which bounds the pass's work per load.
const WALK_LIMIT: usize = 256;
/// The `(constant, register)` loads each loop head's preheader runs, by head pc.
type Preheaders = BTreeMap<usize, Vec<(u32, Register)>>;

/// A loop: the pcs from its head through the back edge at its tail.
#[derive(Clone, Copy)]
struct Loop {
    head: usize,
    tail: usize,
}

impl Loop {
    fn contains(self, pc: usize) -> bool {
        self.head <= pc && pc <= self.tail
    }
}

/// The per-instruction register facts the liveness of the code is derived from.
struct Tables<'a, M: liveness::MethodArguments + ?Sized> {
    methods: &'a M,
    fields: &'a [FieldSite],
    superinstructions: &'a [Superinstruction],
}

impl<M: liveness::MethodArguments + ?Sized> Tables<'_, M> {
    fn uses(&self, instruction: WideInstruction) -> RegisterSet {
        liveness::uses(instruction, self.methods, self.fields, self.superinstructions)
    }

    fn definitions(&self, instruction: WideInstruction) -> RegisterSet {
        liveness::definitions(instruction, self.superinstructions)
    }
}

/// Hoists the loop-invariant `LoadConst`s of `function` (see the module docs). Returns whether
/// any load moved. Runs before `remove_nops`, which drops the loads it `Nop`s.
pub(in crate::compile) fn hoist_loop_constants<M: liveness::MethodArguments + ?Sized>(
    function: &mut BcFunction,
    methods: &M,
    field_sites: &[FieldSite],
    superinstructions: &[Superinstruction],
    scratch: &mut CompileScratch,
) -> bool {
    let usable = usable_loops(function);
    let in_loop_load = function.code.iter().enumerate().any(|(pc, instruction)| {
        instruction.op() == LOAD_CONST && !instruction.is_wide() && usable.iter().any(|lp| lp.contains(pc))
    });
    if !in_loop_load || function.registers >= liveness::ROOT_MAP_REGISTERS {
        return false;
    }
    let Some(analysis) = CodeLiveness::of(&mut None, function, methods, field_sites, superinstructions) else {
        return false;
    };
    let tables = Tables { methods, fields: field_sites, superinstructions };
    let mut hoister = Hoister::new(function, analysis, usable);
    for pc in 0..function.code.len() {
        hoister.try_hoist(function, pc, &tables);
    }
    hoister.apply(function, scratch)
}

/// The loops whose heads only their own code enters, innermost first. A loop is excluded when a
/// jump from outside it targets one of its pcs, or a handler or return target lies in it.
fn usable_loops(function: &BcFunction) -> Vec<Loop> {
    let mut loops: Vec<Loop> = function.back_edges().map(|(tail, head)| Loop { head, tail }).collect();
    loops.sort_unstable_by_key(|lp| (lp.tail - lp.head, lp.head));
    let jumps: Vec<(usize, usize)> = function
        .instructions()
        .enumerate()
        .filter(|(_, instruction)| instruction.op().control_flow_layout().jumps())
        .map(|(pc, instruction)| (pc, instruction.jump_target() as usize))
        .collect();
    let mut entries: Vec<usize> = function
        .handlers
        .iter()
        .flat_map(|handler| std::iter::once(handler.target).chain(handler.return_target))
        .map(|target| target as usize)
        .collect();
    if function.parameter_end_pc != 0 {
        entries.push(function.parameter_end_pc as usize);
    }
    loops
        .into_iter()
        .filter(|&lp| {
            !entries.iter().any(|&entry| lp.contains(entry))
                && jumps.iter().all(|&(source, target)| !lp.contains(target) || lp.contains(source))
        })
        .collect()
}

/// The state of one function's hoisting: the constants hoisted so far and the code they touched.
struct Hoister {
    base: Register,
    usable: Vec<Loop>,
    analysis: CodeLiveness,
    protected: BitSet,
    handled: BitSet,
    parameters: RegisterSet,
    /// The pcs a committed hoist rewrote or dropped, which a later walk does not cross.
    touched: BitSet,
    /// Each hoisted constant with the register that holds it, in allocation order.
    hoisted: Vec<(u32, Register)>,
    preheaders: Preheaders,
}

impl Hoister {
    fn new(function: &BcFunction, analysis: CodeLiveness, usable: Vec<Loop>) -> Self {
        let len = function.code.len();
        let mut handled = BitSet::new(len);
        Handler::for_each_covering(&function.handlers, 0..len, |pc, covering| {
            if !covering.is_empty() {
                handled.insert(pc);
            }
        });
        let parameters = function
            .local_registers
            .iter()
            .filter(|entry| entry.local < function.params)
            .fold(RegisterSet::default(), |mask, entry| mask | register_bit(entry.register));
        Self {
            base: function.registers,
            usable,
            analysis,
            protected: function_protected_positions(function),
            handled,
            parameters,
            touched: BitSet::new(len),
            hoisted: Vec::new(),
            preheaders: Preheaders::new(),
        }
    }

    /// Hoists the load at `pc`, when it is a load a loop can hoist.
    fn try_hoist<M: liveness::MethodArguments + ?Sized>(&mut self, function: &mut BcFunction, pc: usize, tables: &Tables<'_, M>) {
        let packed = function.code[pc];
        if self.touched.contains(pc) || self.handled.contains(pc) || packed.op() != LOAD_CONST || packed.is_wide() || packed.result_flags() != 0 {
            return;
        }
        let destination = packed.result_register();
        // Only an ordinary frame register: the frame's registers are the ones liveness tracks.
        if destination >= self.base || !(register_bit(destination) & self.parameters).is_empty() {
            return;
        }
        let Some(head) = self.usable.iter().copied().find(|lp| lp.contains(pc)).map(|lp| lp.head) else {
            return;
        };
        let constant = packed.constant_index() as u32;
        let Some(register) = self.register_for(constant) else {
            return;
        };
        let walk = Walk {
            function: &*function,
            analysis: &self.analysis,
            touched: &self.touched,
            protected: &self.protected,
            handled: &self.handled,
            destination,
            hoisted: register,
        };
        if let Some(rewrites) = walk.run(pc, tables) {
            self.commit(function, pc, head, constant, register, rewrites);
        }
    }

    /// The register `constant` is hoisted into: its own, or a new one while the budget lasts.
    fn register_for(&self, constant: u32) -> Option<Register> {
        if let Some(&(_, register)) = self.hoisted.iter().find(|(index, _)| *index == constant) {
            return Some(register);
        }
        let slot = self.hoisted.len();
        (slot < MAX_HOISTED_CONSTANTS && usize::from(self.base) + slot < register_limit())
            .then(|| self.base + slot as Register)
    }

    /// Drops the load at `pc` and points its readers at `register`, which preheader `head` loads.
    fn commit(&mut self, function: &mut BcFunction, pc: usize, head: usize, constant: u32, register: Register, rewrites: Vec<(usize, Instr)>) {
        function.code[pc] = Instr::new(NOP, 0, 0, 0, 0);
        self.touched.insert(pc);
        for (at, rewritten) in rewrites {
            function.code[at] = rewritten;
            self.touched.insert(at);
        }
        if !self.hoisted.iter().any(|&(index, _)| index == constant) {
            self.hoisted.push((constant, register));
        }
        let loads = self.preheaders.entry(head).or_default();
        if !loads.contains(&(constant, register)) {
            loads.push((constant, register));
        }
    }

    /// Places the preheaders into `function` and grows its frame by the hoisted registers.
    fn apply(self, function: &mut BcFunction, scratch: &mut CompileScratch) -> bool {
        if self.preheaders.is_empty() {
            return false;
        }
        place_preheaders(function, &self.preheaders, scratch);
        function.registers = self.base + self.hoisted.len() as Register;
        true
    }
}

/// The registers a hoisted constant may take: liveness covers the first `ROOT_MAP_REGISTERS`, and
/// an operand names registers up to the packed domain's bound.
fn register_limit() -> usize {
    liveness::ROOT_MAP_REGISTERS.min(REGISTER_MASK + 1).min(Operand::PAYLOAD_MASK + 1) as usize
}

/// The reads one hoisted load supplies, walked over the code from the load.
struct Walk<'a> {
    function: &'a BcFunction,
    analysis: &'a CodeLiveness,
    touched: &'a BitSet,
    protected: &'a BitSet,
    handled: &'a BitSet,
    destination: Register,
    hoisted: Register,
}

impl Walk<'_> {
    fn bit(&self) -> RegisterSet {
        register_bit(self.destination)
    }

    /// The rewrites that move every read the load at `load` supplies onto the hoisted register,
    /// or `None` when one cannot move or another value reaches a read the load would supply.
    fn run<M: liveness::MethodArguments + ?Sized>(&self, load: usize, tables: &Tables<'_, M>) -> Option<Vec<(usize, Instr)>> {
        let mut rewrites = Vec::new();
        let mut at = load + 1;
        for _ in 0..WALK_LIMIT {
            if at >= self.function.code.len() || self.touched.contains(at) || self.handled.contains(at) {
                return None;
            }
            // Nothing from here reads the register before writing it: the value is dead.
            if (self.analysis.live[at] & self.bit()).is_empty() {
                return Some(rewrites);
            }
            // Control may join here with the register live, from values this walk does not see.
            if self.protected.contains(at) {
                return None;
            }
            if self.function.code[at].op() != NOP {
                let (rewrite, falls_through) = self.visit(at, tables)?;
                rewrites.extend(rewrite);
                if !falls_through {
                    return Some(rewrites);
                }
            }
            at += 1;
        }
        None
    }

    /// The rewrite of the instruction at `at`, if it reads the register, and whether control falls
    /// on past it with the register still live.
    fn visit<M: liveness::MethodArguments + ?Sized>(&self, at: usize, tables: &Tables<'_, M>) -> Option<(Option<(usize, Instr)>, bool)> {
        let packed = self.function.code[at];
        let instruction = self.function.widened(packed);
        let (read, written) = (tables.uses(instruction), tables.definitions(instruction));
        let bit = self.bit();
        let rewrite = if (read & bit).is_empty() {
            None
        } else {
            Some((at, self.redirect(packed, read, written, tables)?))
        };
        if !(written & bit).is_empty() {
            return Some((rewrite, false));
        }
        if instruction.op().control_flow_layout().only_falls_through() {
            return Some((rewrite, true));
        }
        // A branch: the value may still be read along one of its targets.
        if !(liveness::live_out(&self.analysis.successors, &self.analysis.live, at) & bit).is_empty() {
            return None;
        }
        Some((rewrite, false))
    }

    /// `packed` with its reads of the destination moved to the hoisted register, when no other
    /// read or write of the instruction changes.
    fn redirect<M: liveness::MethodArguments + ?Sized>(
        &self,
        packed: Instr,
        read: RegisterSet,
        written: RegisterSet,
        tables: &Tables<'_, M>,
    ) -> Option<Instr> {
        if packed.is_wide() {
            return None;
        }
        let rewritten = substitute_register_read(packed, self.destination, self.hoisted)?;
        let expected = (read & !self.bit()) | register_bit(self.hoisted);
        let after = rewritten.as_wide();
        (tables.uses(after) == expected && tables.definitions(after) == written).then_some(rewritten)
    }
}

/// Rebuilds `function`'s code with each head's preheader placed before the head, relocating the
/// jumps, handlers and positions. Jumps to a head land past its preheader.
fn place_preheaders(function: &mut BcFunction, preheaders: &Preheaders, scratch: &mut CompileScratch) {
    let old = std::mem::take(&mut function.code);
    let mut code = scratch.code_buffer(old.len() + preheaders.values().map(Vec::len).sum::<usize>());
    let map = scratch.remap(old.len());
    for (pc, &packed) in old.iter().enumerate() {
        for &(constant, register) in preheaders.get(&pc).into_iter().flatten() {
            code.push(pack(WideInstruction::new(LOAD_CONST, register, 0, 0, constant), &mut function.wide));
        }
        map[pc] = code.len();
        code.push(packed);
    }
    map[old.len()] = code.len();
    relocate(
        &mut code,
        &mut function.wide,
        &*map,
        &mut function.handlers,
        &mut function.parameter_end_pc,
        &mut function.binding_sites,
        &mut function.source_positions,
        &old,
    );
    function.code = code;
    scratch.recycle_code(old);
}

/// `instruction` in packed form, spilling it to the wide table when the packed domain cannot hold it.
fn pack(instruction: WideInstruction, wide: &mut Vec<WideInstruction>) -> Instr {
    Instr::try_new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm()).unwrap_or_else(|| {
        wide.push(instruction);
        Instr::wide(wide.len() - 1).expect("wide table index fits the packed domain")
    })
}
