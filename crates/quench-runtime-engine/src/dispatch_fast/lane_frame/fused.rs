//! The fusion handlers: a `FUSED_ROWS` pair or a `FUSED_CHAINS` run as one record, and the passes that
//! install them (`fuse_rows`, `fuse_chains`). A child of `lane_frame`, so its monomorphized records
//! land in their own codegen unit and only the lane's build instantiates them (`derive_lane_view`).

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationGenericExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The word a row's double result publishes where it may stay raw (`raw_result`): its raw bits, except a
/// NaN, which keeps the canonical form `LaneFrame::write` gives it (its raw bits could fall in the boxed
/// values' tag space).
#[inline(always)]
fn raw_word(raw: bool, value: u64) -> Value {
    let double = f64::from_bits(value);
    if raw && double.is_nan() { Value::number(double) } else { Value::from_bits(value) }
}

sharded_handler! { view_handler, fused_rows_shards, fused_rows_handler, [A, B];
    /// Two adjacent rows as one record, generated from their staged rows (`staged::FUSED_ROWS`):
    /// row `A` steps over this record, then row `B` over the next one, with no dispatch between
    /// them. Neither row is written for the pair: each is its own row's decode and step, so the
    /// pair is the composition the specializer would make (Futamura). Only this record changes;
    /// the next keeps its own row for jumps into it. A miss of `A` takes `A`'s miss here; a miss
    /// of `B` takes `B`'s at the next record, after `A`'s effects, as two records would.
    /// `BACKWARD` is read only where `B` is a branch table (its entry edges, which no distance gives);
    /// any other jump `B` reads its direction from the record's `mode` (`MODE_BACKWARD`, which the
    /// installer set from the jump's distance), so `fused_row` names one record for both directions.
    /// `FAMILY` is the pair's step family (`FUSED_PLAIN`, `FUSED_WINDOW` or `FUSED_TARGET`); its settings
    /// are bits of the record's `mode` word, read at run time: `MODE_DEAD_FIRST` skips `A`'s register
    /// write (installed only where liveness proves `A`'s register dead and `B` does not name it);
    /// `raw_bit(0)` keeps `A`'s double result as its bits (`LaneFrame::raw_result`), which `B`, a Number
    /// operator, reads as its operand; `MODE_RAW_OUT` keeps `B`'s own double result raw for the next record.
    /// `FUSED_TARGET` (a backward `B` whose target run holds: `PREFIX` (or none) then the test `TEST`):
    /// the back edge's fuel check and the target run in this record (`target_run`), which continues at
    /// the test's taken or fall-through edge. Any other family ignores `PREFIX` and `TEST`. `CMP`, where
    /// not 0, fixes the test's comparison and operand form at install (`target_comparison`), so the
    /// back edge runs that one test with no switch on the record's operator. A `FUSED_PLAIN` pair's
    /// `CMP` is always 0: a `Binary` of a Number operator is never paired here, it runs its own operator row.
    #[inline(never)]
    pub(crate) fn fused_rows<A: u16, B: u16, BACKWARD: bool, FAMILY: u8, TEST: u16, PREFIX: u16, CMP: u8>(cx, r, ip, i, view) {
        // Both rows of the pair come from one program: `A`'s opcode names the view its records
        // are dispatched from (`row_context`).
        let cx: *mut LaneContext = row_context::<A>(cx);
        // The settings word of this record (the pair's head), read before `B`'s record is named.
        let mode = i.mode;
        // A proven loop step (`PROVEN_STEP` of `CMP`, `prove_loop_steps`): `A` is `i++` of a register the
        // solve's int32 cells prove (`int32_cell`), read with no tag test, and the test's left operand is
        // that register, whose new value this record holds. Its decrement flag is clear.
        let proven = const { FAMILY == quench_runtime::staged::FUSED_TARGET && CMP & PROVEN_STEP != 0 };
        #[allow(unused_assignments)] let mut held: u64 = 0;
        // Row `A` at flag `$flag`: a flagged row (`LaneFields::FLAG_ROLE`) reads its flag here,
        // from the record, rather than requickening (`generated_row`).
        macro_rules! step_a { ($flag:expr) => {{
            let mut stage = quench_runtime::staged::concrete_over_vm(unsafe { vm(cx) });
            let mut frame = row_frame::<A, ACC_NONE, { $flag }>(cx, r, ip, i, view);
            let raw_result = mode & quench_runtime::staged::raw_bit(0) != 0;
            frame.raw_result = raw_result;
            if proven { frame.operands_int = true; }
            let fields = LaneFields::<A, { $flag }>(i);
            let Some(operation) = row_decode!(A, { $flag }, fields) else { sibling!(general_step_in_lane, r, ip, view) };
            let step = row_step!(LaneFields::<A>::OPCODE, &mut stage, &mut frame, operation);
            match step {
                Step::Value(value) => {
                    held = value;
                    if const { !quench_runtime::staged::row_writes_result(LaneFields::<A>::OPCODE) } && mode & quench_runtime::staged::MODE_DEAD_FIRST == 0 {
                        r.set(i.a, raw_word(raw_result, value));
                    }
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<{ $flag }>(LaneFields::<A>::OPCODE) }, r, ip, view),
            }
        }}; }
        if proven && LaneFields::<A>::FLAG_ROLE {
            step_a!(FLAG_CLEAR)
        } else if LaneFields::<A>::FLAG_ROLE {
            if i.imm() != 0 { step_a!(FLAG_SET) } else { step_a!(FLAG_CLEAR) }
        } else if const { FAMILY == quench_runtime::staged::FUSED_WINDOW } {
            step_a!(FLAG_WINDOW)
        } else {
            step_a!(FLAG_UNREAD)
        }
        // `B` over the next record; it reads registers (no accumulator), which `A` wrote.
        let ip = following(ip);
        // SAFETY: the next record of the running view.
        let i = unsafe { &*ip };
        let fields = LaneFields::<B>(i);
        if const { jumps_by_row(LaneFields::<B>::OPCODE) } {
            let decoded = if const { B == quench_runtime::staged::COMPARE_JUMP_ROW } {
                compare_fields(fields)
            } else if const { matches!(LaneFields::<B>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Names)) } {
                Operation::decode_names(quench_runtime::staged::Fields::wide(fields))
            } else if const { matches!(LaneFields::<B>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::WasmJump)) } {
                Operation::decode_wasm_jump(quench_runtime::staged::Fields::wide(fields))
            } else {
                Operation::decode_flow(fields, quench_runtime::staged::DecodeContext::EMPTY)
            };
            let Some(operation) = decoded else { sibling!(general_step_in_lane, r, ip, view) };
            let mut stage = Concrete::pure();
            let carried = if const { LaneFields::<B>::RESULT_A } { Carried::past_result(view) } else { view };
            // The jump's direction: a branch table's is its `BACKWARD` (entry edges); any other jump's is the record's `MODE_BACKWARD`.
            let backward = if const { quench_runtime::staged::wasm_branch_table(LaneFields::<B>::OPCODE) } { BACKWARD } else { mode & quench_runtime::staged::MODE_BACKWARD != 0 };
            let mut frame = row_frame::<B, ACC_NONE, FLAG_UNREAD>(cx, r, ip, i, view);
            match row_step!(LaneFields::<B>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Control(LaneControl::Edge(Edge::Target)) => {
                    if const { FAMILY == quench_runtime::staged::FUSED_TARGET } {
                        if proven {
                            target_run!(cx, r, ip, carried, PREFIX, NO_COPY, TEST, CMP & !PROVEN_STEP, held)
                        } else {
                            target_run!(cx, r, ip, carried, PREFIX, NO_COPY, TEST, { CMP & !PROVEN_STEP })
                        }
                    } else {
                        resume!(cx, crate::dispatch_fast::take_jump(cx, r, ip, backward, carried, None))
                    }
                }
                Step::Control(LaneControl::Edge(Edge::Entry(k))) => resume!(cx, crate::dispatch_fast::take_jump(cx, r, unsafe { ip.add(1 + k as usize) }, backward, carried, None)),
                Step::Control(LaneControl::Edge(Edge::Leave)) => unreachable!("a jump leaves no lane run"),
                Step::Control(LaneControl::Resumed(_) | LaneControl::Suspend(_) | LaneControl::Edge(Edge::Transfer)) => unreachable!("a jump transfers no control"),
                Step::Control(LaneControl::Edge(Edge::Following)) | Step::Value(_) => next!(cx, r, following(ip), carried),
                Step::Unresolved => if backward {
                    sibling!(const { jump_row_miss::<true>(LaneFields::<B>::OPCODE) }, r, ip, view)
                } else {
                    sibling!(const { jump_row_miss::<false>(LaneFields::<B>::OPCODE) }, r, ip, view)
                },
            }
        }
        macro_rules! step_b { ($flag:expr) => {{
            let fields = LaneFields::<B, { $flag }>(i);
            let Some(operation) = row_decode!(B, { $flag }, fields) else { sibling!(general_step_in_lane, r, ip, view) };
            let mut stage = quench_runtime::staged::concrete_over_vm(unsafe { vm(cx) });
            let mut frame = row_frame::<B, ACC_NONE, { $flag }>(cx, r, ip, i, view);
            // `B` is the last link: its double result stays raw where the next record reads it (`MODE_RAW_OUT`).
            let raw_result = mode & quench_runtime::staged::MODE_RAW_OUT != 0;
            frame.raw_result = raw_result;
            match row_step!(LaneFields::<B>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !quench_runtime::staged::row_writes_result(LaneFields::<B>::OPCODE) } {
                        r.set(i.a, raw_word(raw_result, value));
                    }
                    if const { row_produces(LaneFields::<B>::OPCODE) } {
                        // A raw result's NaN keeps its canonical form, as `LaneFrame::write` gives it (`raw_word`).
                        let acc = raw_word(raw_result, value);
                        next!(cx, r, following(ip), Carried { acc: Slot::of(acc), ..view })
                    }
                    next!(cx, r, following(ip), view)
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<{ $flag }>(LaneFields::<B>::OPCODE) }, r, ip, view),
            }
        }}; }
        if LaneFields::<B>::FLAG_ROLE {
            if i.imm() != 0 { step_b!(FLAG_SET) } else { step_b!(FLAG_CLEAR) }
        } else if const { FAMILY == quench_runtime::staged::FUSED_WINDOW } {
            step_b!(FLAG_WINDOW)
        } else {
            step_b!(FLAG_UNREAD)
        }
    }
}

sharded_handler! { view_handler, chain_rows_shards, chain_rows_handler, [L0, L1, L2, L3];
    /// Three or four adjacent rows as one record, generated from their staged rows
    /// (`staged::FUSED_CHAINS`): each link is its row's own decode and step over its record,
    /// with no dispatch between links, as `fused_rows` does for two. A link's miss takes its
    /// row's miss at its own record, after the earlier links' effects, as separate records
    /// would. Only the last link may be a jump (its direction is the record's `MODE_BACKWARD`). `L3` is
    /// `staged::NO_LINK` for a three-link chain. `FAMILY` is the chain's step family (`FUSED_PLAIN` or
    /// `FUSED_WINDOW`). The head record's `mode` word holds the raw mask: bit `k` (`raw_bit`) keeps link `k`'s
    /// double result raw (`LaneFrame::raw_result`), which the next link reads as its operand or key.
    #[inline(never)]
    pub(crate) fn chain_rows<L0: u16, L1: u16, L2: u16, L3: u16, FAMILY: u8>(cx, r, ip, i, view) {
        // Every link of the chain comes from one program: `L0`'s opcode names the view.
        let cx: *mut LaneContext = row_context::<L0>(cx);
        // The head record's settings word, read before the links name their own records.
        let mode = i.mode;
        // Each macro takes its link's record (`$ip`, `$i`) as arguments: a `macro_rules!` body's own
        // `ip` and `i` would name the head record's (the handler's parameters, which its definition
        // sees), not the link's that the `let`s below shadow them with.
        // Link `$k` at flag `$flag`; its double result is raw where `mode`'s raw mask has bit `$k`.
        macro_rules! link_flag { ($ip:ident, $i:ident, $L:ident, $k:literal, $flag:expr) => {{
            let fields = LaneFields::<$L, { $flag }>($i);
            let Some(operation) = row_decode!($L, { $flag }, fields) else { sibling!(general_step_in_lane, r, $ip, view) };
            let mut stage = quench_runtime::staged::concrete_over_vm(unsafe { vm(cx) });
            let mut frame = row_frame::<$L, ACC_NONE, { $flag }>(cx, r, $ip, $i, view);
            let raw_result = mode & quench_runtime::staged::raw_bit($k) != 0;
            frame.raw_result = raw_result;
            match row_step!(LaneFields::<$L>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !quench_runtime::staged::row_writes_result(LaneFields::<$L>::OPCODE) } {
                        r.set($i.a, raw_word(raw_result, value));
                    }
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<{ $flag }>(LaneFields::<$L>::OPCODE) }, r, $ip, view),
            }
        }}; }
        macro_rules! value_link { ($ip:ident, $i:ident, $L:ident, $k:literal) => {
            if LaneFields::<$L>::FLAG_ROLE {
                if $i.imm() != 0 { link_flag!($ip, $i, $L, $k, FLAG_SET) } else { link_flag!($ip, $i, $L, $k, FLAG_CLEAR) }
            } else {
                if const { FAMILY == quench_runtime::staged::FUSED_WINDOW } { link_flag!($ip, $i, $L, $k, FLAG_WINDOW) } else { link_flag!($ip, $i, $L, $k, FLAG_UNREAD) }
            }
        }; }
        macro_rules! last_flag { ($ip:ident, $i:ident, $L:ident, $flag:expr) => {{
            let fields = LaneFields::<$L, { $flag }>($i);
            let Some(operation) = row_decode!($L, { $flag }, fields) else { sibling!(general_step_in_lane, r, $ip, view) };
            let mut stage = quench_runtime::staged::concrete_over_vm(unsafe { vm(cx) });
            let mut frame = row_frame::<$L, ACC_NONE, { $flag }>(cx, r, $ip, $i, view);
            // The last link's double result stays raw where the next record reads it (`CHAIN_RAW_OUT`, `MODE_RAW_OUT`).
            let raw_result = mode & quench_runtime::staged::MODE_RAW_OUT != 0;
            frame.raw_result = raw_result;
            match row_step!(LaneFields::<$L>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !quench_runtime::staged::row_writes_result(LaneFields::<$L>::OPCODE) } {
                        r.set($i.a, raw_word(raw_result, value));
                    }
                    if const { row_produces(LaneFields::<$L>::OPCODE) } {
                        // A raw result's NaN keeps its canonical form, as `LaneFrame::write` gives it (`raw_word`).
                        let acc = raw_word(raw_result, value);
                        next!(cx, r, following($ip), Carried { acc: Slot::of(acc), ..view })
                    }
                    next!(cx, r, following($ip), view)
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<{ $flag }>(LaneFields::<$L>::OPCODE) }, r, $ip, view),
            }
        }}; }
        macro_rules! last_link { ($ip:ident, $i:ident, $L:ident) => {{
        let fields = LaneFields::<$L>($i);
            if const { jumps_by_row(LaneFields::<$L>::OPCODE) } {
                let decoded = if const { $L == quench_runtime::staged::COMPARE_JUMP_ROW } {
                    compare_fields(fields)
                } else if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Names)) } {
                    Operation::decode_names(quench_runtime::staged::Fields::wide(fields))
                } else if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::WasmJump)) } {
                    Operation::decode_wasm_jump(quench_runtime::staged::Fields::wide(fields))
                } else {
                    Operation::decode_flow(fields, quench_runtime::staged::DecodeContext::EMPTY)
                };
                let Some(operation) = decoded else { sibling!(general_step_in_lane, r, $ip, view) };
                let mut stage = Concrete::pure();
                // A chain's last link is never a branch table, so its direction is the record's `MODE_BACKWARD`.
                let backward = mode & quench_runtime::staged::MODE_BACKWARD != 0;
                let carried = if const { LaneFields::<$L>::RESULT_A } { Carried::past_result(view) } else { view };
                let mut frame = row_frame::<$L, ACC_NONE, FLAG_UNREAD>(cx, r, $ip, $i, view);
                match row_step!(LaneFields::<$L>::OPCODE, &mut stage, &mut frame, operation) {
                    Step::Control(LaneControl::Edge(Edge::Target)) => resume!(cx, crate::dispatch_fast::take_jump(cx, r, $ip, backward, carried, None)),
                    Step::Control(LaneControl::Edge(Edge::Entry(k))) => resume!(cx, crate::dispatch_fast::take_jump(cx, r, unsafe { $ip.add(1 + k as usize) }, backward, carried, None)),
                    Step::Control(LaneControl::Edge(Edge::Leave)) => unreachable!("a jump leaves no lane run"),
                    Step::Control(LaneControl::Resumed(_) | LaneControl::Suspend(_) | LaneControl::Edge(Edge::Transfer)) => unreachable!("a jump transfers no control"),
                    Step::Control(LaneControl::Edge(Edge::Following)) | Step::Value(_) => next!(cx, r, following($ip), carried),
                    Step::Unresolved => if backward {
                        sibling!(const { jump_row_miss::<true>(LaneFields::<$L>::OPCODE) }, r, $ip, view)
                    } else {
                        sibling!(const { jump_row_miss::<false>(LaneFields::<$L>::OPCODE) }, r, $ip, view)
                    },
                }
            }
            if LaneFields::<$L>::FLAG_ROLE {
                if $i.imm() != 0 { last_flag!($ip, $i, $L, FLAG_SET) } else { last_flag!($ip, $i, $L, FLAG_CLEAR) }
            } else {
                if const { FAMILY == quench_runtime::staged::FUSED_WINDOW } { last_flag!($ip, $i, $L, FLAG_WINDOW) } else { last_flag!($ip, $i, $L, FLAG_UNREAD) }
            }
        }}; }
        value_link!(ip, i, L0, 0);
        let ip = following(ip);
        // SAFETY: the next record of the running view (the chain's install checked its length).
        let i = unsafe { &*ip };
        value_link!(ip, i, L1, 1);
        let ip = following(ip);
        // SAFETY: as above. Each link names its own record: a macro body sees only the bindings
        // in scope where it was defined, so the record is passed in, not shadowed.
        let i = unsafe { &*ip };
        if const { L3 == quench_runtime::staged::NO_LINK } {
            last_link!(ip, i, L2)
        } else {
            value_link!(ip, i, L2, 2);
            let ip = following(ip);
            let i = unsafe { &*ip };
            last_link!(ip, i, L3)
        }
    }
}

/// Chain `(L0, L1, L2, L3)`'s record handler: one for both jump directions of its last link, which reads its own distance.
pub(crate) const fn chain_row<const L0: u16, const L1: u16, const L2: u16, const L3: u16, const FAMILY: u8>() -> Handler {
    chain_rows!(L0, L1, L2, L3, FAMILY)
}

/// Whether `handler` is opcode `OP`'s window row (`FLAG_WINDOW`): a local record of a plain slot in
/// an environment window's function, which `window_local_handler` gives its row.
pub(crate) fn is_window_row<const OP: u16>(handler: *const ()) -> bool {
    generated_row!(OP, ACC_NONE, FLAG_WINDOW) as Handler as *const () == handler
}

/// `chains!` index `chain` within `staged::FUSED_CHAINS` (past its end, entry 0 again).
const fn chain_index(chain: usize) -> usize { if chain < quench_runtime::staged::FUSED_CHAINS.len() { chain } else { 0 } }

/// Whether the double result of the producer at `producer` keeps its raw bits (a chain's raw link):
/// the next record reads that register as an operand, and the register is dead after the next record
/// or the next record writes it. A keyed element access (`keyed`) reads it as its key alone, so its
/// base and a store's value must not name the register. Liveness is asked only where the link shape holds.
fn chain_link_raw(records: &[LaneInstruction], live: Option<&quench_runtime::compile::liveness::RegisterMasks>, producer: usize, keyed: bool) -> bool {
    let (first, second) = (&records[producer], &records[producer + 1]);
    let names = |field: u16| Operand(field).register_index() == Some(first.a);
    let dead_after = || live.and_then(|masks| masks.get(producer + 2))
        .is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(first.a)));
    if keyed {
        names(second.c) && !names(second.b) && second.a != first.a && dead_after()
    } else {
        (names(second.b) || names(second.c)) && (second.a == first.a || dead_after())
    }
}

/// Records a raw result may pass over between its producer and its consumer (`raw_out_link`): the
/// plain increment and element read rows (`plain_row`), at any flag. Neither jumps nor calls, and a
/// jump row or a call row never matches these two opcodes' plain rows.
fn passed_row(handler: *const ()) -> bool {
    plain_row::<{ quench_runtime::staged::INC_DEC_ROW }>(handler) || plain_row::<{ quench_runtime::staged::ELEMENT_READ_ROW }>(handler)
}

/// The jump targets of a function's decoded instructions, or `None` where one cannot be named: a Wasm
/// branch table's entries are not its immediate, so no record between two others is then known to be
/// unentered. A `JumpTarget` immediate names its target.
fn jump_targets(decoded: &[quench_runtime::bytecode::WideInstruction]) -> Option<quench_runtime::bitset::BitSet> {
    let mut targets = quench_runtime::bitset::BitSet::new(decoded.len());
    for instruction in decoded {
        let op = instruction.op();
        if quench_runtime::staged::wasm_branch_table(op) { return None; }
        if !op.control_flow_layout().jumps() { continue; }
        if op.immediate_role() != quench_runtime::bytecode::ImmediateRole::JumpTarget { return None; }
        let target = usize::try_from(instruction.jump_target()).ok()?;
        if target >= decoded.len() { return None; }
        targets.insert(target);
    }
    Some(targets)
}

/// How many records `raw_out_link` passes over at most, past the producer's next record.
const RAW_PASS: usize = 6;

/// Whether a fused record's last link `producer` keeps its double result raw (`MODE_RAW_OUT`):
/// the first record after it that names the producer's register is a Number binary row (`number_binary_row`)
/// that reads the register as an operand, and the register is dead after that record or that record writes it.
/// The records between them name neither the register nor anything else that could read it: each is a
/// `passed_row`, not a jump target (`targets`, from `jump_targets`), and none is a jump or a call. Its raw
/// bits are then only ever read as the Number operand of that row, which reads a raw double as a Number
/// (`number_operand`). `targets` is `None` where the function's jump targets are unknown: no record passes.
fn raw_out_link(
    records: &[LaneInstruction],
    live: Option<&quench_runtime::compile::liveness::RegisterMasks>,
    targets: Option<&quench_runtime::bitset::BitSet>,
    producer: usize,
) -> bool {
    let Some(targets) = targets else { return false };
    let register = records[producer].a;
    let names = |field: u16| field == register || Operand(field).register_index() == Some(register);
    let names_record = |record: &LaneInstruction| names(record.a) || names(record.b) || names(record.c);
    let mut consumer = producer + 1;
    loop {
        let Some(record) = records.get(consumer) else { return false };
        if names_record(record) { break; }
        if consumer - producer > RAW_PASS || !passed_row(record.handler) || targets.contains(consumer) {
            return false;
        }
        consumer += 1;
    }
    let next = &records[consumer];
    let reads = [next.b, next.c].into_iter().any(|field| Operand(field).register_index() == Some(register));
    reads && number_binary_row(next.handler)
        && (next.a == register || live.and_then(|masks| masks.get(consumer + 1))
            .is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(register))))
}

/// Whether `handler` is a plain Number binary row (`staged::BINARY_ROW`, `NUMERIC_ADD_ROW`, `NUMERIC_MULTIPLY_ROW`):
/// the rows a raw result may feed (`raw_out_link`).
fn number_binary_row(handler: *const ()) -> bool {
    plain_row::<{ quench_runtime::staged::BINARY_ROW }>(handler)
        || plain_row::<{ quench_runtime::staged::NUMERIC_ADD_ROW }>(handler)
        || plain_row::<{ quench_runtime::staged::NUMERIC_MULTIPLY_ROW }>(handler)
}

/// Gives each record that starts a run of records whose rows are a chain of
/// `staged::FUSED_CHAINS` the chain's record (`chain_rows`). Runs before `fuse_rows`, so a
/// chain wins over the pair at its head; the later records keep their own rows for jumps
/// into them. A chain whose middle record is a jump target still fuses: the jump enters at
/// that record's own row. A link keeps its double result raw (its `mode` raw mask, `raw_bit`) where
/// `chain_raw_links` allows it and `chain_link_raw` holds for the record; `raw_results` (no
/// handler in the function) gates it, as for `fuse_rows`. Returns each fused record's head and last record.
pub(crate) fn fuse_chains(records: &mut [LaneInstruction], decoded: &[quench_runtime::bytecode::WideInstruction], live: Option<&quench_runtime::compile::liveness::RegisterMasks>, raw_results: bool) -> Vec<(usize, usize)> {
    if const { quench_runtime::staged::FUSED_CHAINS.is_empty() } { return Vec::new(); }
    // The jump targets a raw result may not pass over (`raw_out_link`); a function with a handler has no raw results.
    let targets = if raw_results { jump_targets(decoded) } else { None };
    macro_rules! link { ($chain:literal, $k:literal) => { quench_runtime::staged::FUSED_CHAINS[chain_index($chain)][$k] }; }
    // Chain `$chain`'s record handler for its step family (`FUSED_PLAIN`, or `FUSED_WINDOW` for a window chain).
    // Its raw mask and direction are the record's `mode` word, written per record below, so one instance serves every mask.
    macro_rules! chains {
        ($($chain:literal),*) => {
            [$((
                [
                    (|handler| plain_row::<{ link!($chain, 0) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| plain_row::<{ link!($chain, 1) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| plain_row::<{ link!($chain, 2) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| if const { link!($chain, 3) == quench_runtime::staged::NO_LINK } { true } else { plain_row::<{ link!($chain, 3) }>(handler) }) as fn(*const ()) -> bool,
                ],
                if link!($chain, 3) == quench_runtime::staged::NO_LINK { 3usize } else { 4 },
                chain_row::<{ link!($chain, 0) }, { link!($chain, 1) }, { link!($chain, 2) }, { link!($chain, 3) }, { quench_runtime::staged::FUSED_PLAIN }>(),
                quench_runtime::staged::chain_raw_eligible(quench_runtime::staged::FUSED_CHAINS[chain_index($chain)]),
                quench_runtime::staged::chain_keyed_links(quench_runtime::staged::FUSED_CHAINS[chain_index($chain)]),
                quench_runtime::staged::chain_last_row(quench_runtime::staged::FUSED_CHAINS[chain_index($chain)]),
            )),*]
        };
    }
    // The window chains: a chain whose first row is a window local's (`FLAG_WINDOW`) installs its window
    // record (`FUSED_WINDOW`), whose links all run at the window's flag. The indices name `FUSED_CHAINS`.
    macro_rules! window_chains {
        ($($chain:literal),*) => {
            [$((
                [
                    (|handler| is_window_row::<{ link!($chain, 0) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| plain_row::<{ link!($chain, 1) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| plain_row::<{ link!($chain, 2) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| if const { link!($chain, 3) == quench_runtime::staged::NO_LINK } { true } else { plain_row::<{ link!($chain, 3) }>(handler) }) as fn(*const ()) -> bool,
                ],
                if link!($chain, 3) == quench_runtime::staged::NO_LINK { 3usize } else { 4 },
                chain_row::<{ link!($chain, 0) }, { link!($chain, 1) }, { link!($chain, 2) }, { link!($chain, 3) }, { quench_runtime::staged::FUSED_WINDOW }>(),
                quench_runtime::staged::chain_raw_eligible(quench_runtime::staged::FUSED_CHAINS[chain_index($chain)]),
                quench_runtime::staged::chain_keyed_links(quench_runtime::staged::FUSED_CHAINS[chain_index($chain)]),
                quench_runtime::staged::chain_last_row(quench_runtime::staged::FUSED_CHAINS[chain_index($chain)]),
            )),*]
        };
    }
    let window_chains = window_chains!(0, 1);
    let chains = chains!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63);
    const { assert!(quench_runtime::staged::FUSED_CHAINS.len() <= 64, "`chains!` covers 64 chains; add indices") };
    let mut fused_records = Vec::new();
    for pc in 0..records.len() {
        let Some((_, length, handler, eligible, keyed, last_row)) = chains.iter().chain(window_chains.iter()).find(|(links, length, _, _, _, _)| {
            pc + length <= records.len() && (0..*length).all(|k| links[k](records[pc + k].handler))
        }) else { continue };
        let (length, eligible, keyed) = (*length, *eligible, *keyed);
        // The raw mask: each eligible link whose producer's register the next record takes raw.
        let mut raw = 0usize;
        if raw_results {
            for k in 0..length - 1 {
                if eligible & (1 << k) != 0 && chain_link_raw(records, live, pc + k, keyed & (1 << k) != 0) { raw |= 1 << k; }
            }
            // The last link's raw-out bit (`CHAIN_RAW_OUT`): the record after the chain reads it as a Number.
            let out = quench_runtime::staged::CHAIN_RAW_OUT;
            if eligible & (1 << out) != 0 && raw_out_link(records, live, targets.as_ref(), pc + length - 1) { raw |= 1 << out; }
        }
        // A chain whose last link is a jump takes its direction from that record's distance (`MODE_BACKWARD`).
        let backward = if jumps_by_row(quench_runtime::staged::row_op(*last_row)) && records[pc + length - 1].jump() <= 0 { quench_runtime::staged::MODE_BACKWARD } else { 0 };
        records[pc].handler = *handler as *const ();
        records[pc].mode = quench_runtime::staged::raw_mask_word(raw as u8) | backward;
        fused_records.push((pc, pc + length - 1));
    }
    fused_records
}

/// Opcode pair `(A, B)`'s fused record handler for `B`'s jump direction `backward`. Only a branch table
/// second row reads the direction (`fused_rows`' `BACKWARD`); any other `B` takes its own distance, so
/// both directions name one record and one instance.
pub(crate) fn fused_row<const A: u16, const B: u16, const FAMILY: u8>(backward: bool) -> Handler {
    if const { quench_runtime::staged::wasm_branch_table(LaneFields::<B>::OPCODE) } {
        if backward { fused_rows!(A, B, true, FAMILY, B, { quench_runtime::staged::NO_LINK }, 0) } else { fused_rows!(A, B, false, FAMILY, B, { quench_runtime::staged::NO_LINK }, 0) }
    } else {
        fused_rows!(A, B, false, FAMILY, B, { quench_runtime::staged::NO_LINK }, 0)
    }
}

/// `pairs!` index `pair` within `staged::FUSED_ROWS`: indices past its end repeat entry 0, so
/// the table grows without touching the macro (a repeated entry never matches first).
const fn fused_index(pair: usize) -> usize { if pair < quench_runtime::staged::FUSED_ROWS.len() { pair } else { 0 } }

/// Gives each record whose row and next record's row are a pair of `staged::FUSED_ROWS` the
/// pair's fused record (`fused_rows`); the next record keeps its own row. A `FUSED_DEAD_FIRST`
/// pair takes its fused record only where `liveness` proves `A`'s result register dead after
/// the pair and `B` names that register in no operand; otherwise it takes the `FUSED_PLAIN` record.
/// A pair whose `B` is a Number operator takes its raw-out settings (`MODE_RAW_OUT`) where `raw_out_link` holds.
/// A `FUSED_RAW` pair takes its raw record only where `raw_results` (no handler in the function) and
/// `B` reads `A`'s register with `A`'s register dead after it or overwritten by `B`.
/// Returns each fused record's head and its last record, which `derive_lane_view` reads for the accumulator carry.
pub(crate) fn fuse_rows(records: &mut [LaneInstruction], decoded: &[quench_runtime::bytecode::WideInstruction], live: Option<&quench_runtime::compile::liveness::RegisterMasks>, osr_loop: bool, raw_results: bool) -> Vec<(usize, usize)> {
    // Pair `$pair`'s record handlers for its step family, indexed by the direction (`fused_row`). Its table
    // mode is no instance: `fuse_rows` writes it as the record's settings bits, so one family instance serves
    // every mode of the pair.
    macro_rules! pairs {
        ($($pair:literal),*) => {
            [$((
                (|handler| plain_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }>(handler)) as fn(*const ()) -> bool,
                (|handler| plain_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }>(handler)) as fn(*const ()) -> bool,
                jump_row_backward::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }> as fn(*const ()) -> bool,
                quench_runtime::staged::FUSED_ROWS[fused_index($pair)].2,
                // Both rows are Number operators: a raw `A` result is then only a Number's operand.
                const { quench_runtime::staged::number_operator(LaneFields::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }>::OPCODE)
                    && quench_runtime::staged::number_operator(LaneFields::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }>::OPCODE) },
                // `B` is a Number operator, so its result may be a raw-out (`MODE_RAW_OUT`) producer.
                const { quench_runtime::staged::number_operator(LaneFields::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }>::OPCODE) },
                [fused_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }, { quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }, { quench_runtime::staged::FUSED_PLAIN }>(false),
                    fused_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }, { quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }, { quench_runtime::staged::FUSED_PLAIN }>(true)],
            )),*]
        };
    }
    // The window pairs: a pair whose first row is a window local's installs its window record
    // (`FUSED_WINDOW`). The indices name `FUSED_ROWS` (its plain local-read and local-store pairs with a jump).
    macro_rules! window_pairs {
        ($($pair:literal),*) => {
            [$((
                (|handler| is_window_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }>(handler)) as fn(*const ()) -> bool,
                (|handler| plain_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }>(handler)) as fn(*const ()) -> bool,
                jump_row_backward::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }> as fn(*const ()) -> bool,
                [fused_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }, { quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }, { quench_runtime::staged::FUSED_WINDOW }>(false),
                    fused_row::<{ quench_runtime::staged::FUSED_ROWS[fused_index($pair)].0 }, { quench_runtime::staged::FUSED_ROWS[fused_index($pair)].1 }, { quench_runtime::staged::FUSED_WINDOW }>(true)],
            )),*]
        };
    }
    let window_pairs = window_pairs!(31, 34, 35, 63);
    let pairs = pairs!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127);
    const { assert!(quench_runtime::staged::FUSED_ROWS.len() <= 128, "`pairs!` covers 128 fused pairs; add indices") };
    // The jump targets a raw-out record may not pass over (`raw_out_link`).
    let targets = if raw_results { jump_targets(decoded) } else { None };
    let mut fused_records = Vec::new();
    for pc in 0..records.len().saturating_sub(1) {
        let (first, second) = (records[pc].handler, records[pc + 1].handler);
        if let Some((_, _, backward, window)) = window_pairs.iter().find(|(a, b, _, _)| a(first) && b(second)) {
            let direction = usize::from(backward(second));
            records[pc].handler = window[direction] as *const ();
            records[pc].mode = if direction == 1 { quench_runtime::staged::MODE_BACKWARD } else { 0 };
            fused_records.push((pc, pc + 1));
            continue;
        }
        let Some((_, _, backward, mode, numbers, number_b, fused)) = pairs.iter().find(|(a, b, _, _, _, _, _)| a(first) && b(second)) else {
            continue;
        };
        let direction = usize::from(backward(second));
        // A move or an add of a constant at a hot OSR latch stays its own record, which
        // `install_loop_sites` read as the latch's (`derive_lane_view`'s `then_fuses`).
        if osr_loop && direction == 1 && (plain_row::<{ quench_runtime::staged::MOVE_ROW }>(first) || plain_row::<{ quench_runtime::staged::WASM_I32_ADD_IMMEDIATE_ROW }>(first)) {
            continue;
        }
        // The dead-first form is only sound when `B` never reads `A`'s register, and nothing
        // after the pair reads it. `B`'s `a` is a read when `B` is a jump, so it counts as a read
        // here too; a value `B` that overwrites `a` is simply not claimed dead (conservative).
        // A jump `B`'s target is not checked, so `FUSED_DEAD_FIRST` names value rows only.
        let dead_first = {
            let (first, second) = (&records[pc], &records[pc + 1]);
            second.a != first.a
                && second.b != first.a
                && second.c != first.a
                && live
                    .and_then(|masks| masks.get(pc + 2))
                    .is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(first.a)))
        };
        // A `FUSED_RAW` pair keeps `A`'s double result raw only where both rows are Number operators
        // (`numbers`), `B` reads `A`'s register as an operand, and nothing after the pair reads it
        // (`B` may write it instead). Its raw bits are then only ever a Number operand. A function with
        // a handler never does (`raw_results`): a catch may read any register.
        let raw = raw_results && *mode == quench_runtime::staged::FUSED_RAW && *numbers && {
            let (producer, consumer) = (&records[pc], &records[pc + 1]);
            let reads = [consumer.b, consumer.c].into_iter().any(|field| Operand(field).register_index() == Some(producer.a));
            reads && (consumer.a == producer.a
                || live.and_then(|masks| masks.get(pc + 2)).is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(producer.a))))
        };
        // `B`'s raw-out: its double result feeds the next record as a raw Number (`raw_out_link`).
        let out = raw_results && *number_b && raw_out_link(records, live, targets.as_ref(), pc + 1);
        // The record's settings: a `FUSED_DEAD_FIRST` pair skips `A`'s write only where liveness proves it
        // (`dead_first`), a `FUSED_RAW` pair keeps `A`'s result raw only where `raw` holds, and `B`'s raw-out
        // bit is `out`. Each is clear where its table mode does not hold, which is the staged pair without it.
        let mut word = if direction == 1 { quench_runtime::staged::MODE_BACKWARD } else { 0 };
        if *mode == quench_runtime::staged::FUSED_DEAD_FIRST && dead_first { word |= quench_runtime::staged::MODE_DEAD_FIRST; }
        if *mode == quench_runtime::staged::FUSED_RAW && raw { word |= quench_runtime::staged::raw_bit(0); }
        if out { word |= quench_runtime::staged::MODE_RAW_OUT; }
        records[pc].handler = fused[direction] as *const ();
        records[pc].mode = word;
        fused_records.push((pc, pc + 1));
    }
    fused_records
}
