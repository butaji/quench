//! Resume stubs: where a run that left the region's PC (a Cranelift body's guard, say) re-enters
//! its stencil code. A stub is the entry rows for the webs live at its PC (each pinned register
//! loaded and checked from its frame slot, as at the header) and a jump to the first row of the PC's
//! instruction (of a stitched program, `Layout::origins`);
//! the table names each stub's code offset, and the resume thunk (`entry_resume`) calls it
//! through the shared entry ABI, so a register that holds no Number there leaves with
//! `LaneExit::Entry` and the interpreter resumes at the PC.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_bjit::HOLES;
use super::link::Target;

pub(crate) use quench_runtime::vm::engine_types::ResumeTable;

/// A code offset as the table keeps it.
fn offset_word(offset: usize) -> Result<u32, String> {
    u32::try_from(offset).map_err(|_| format!("code offset {offset} past the table's reach"))
}

#[quench_macros::ext(ResumeTableOps1)]
impl ResumeTable {
    /// The table of `stubs`, whose first rows `jumps` name, over the code `offsets` of the rows.
    pub(crate) fn of(thunk: usize, stubs: &[Stub], jumps: &[Jump], offsets: &[usize]) -> Result<Self, String> {
        let entries = stubs.iter().zip(jumps).map(|(stub, jump)| Ok((stub.pc, offset_word(offsets[jump.first])?)));
        let (pcs, offsets): (Vec<u32>, Vec<u32>) = entries.collect::<Result<Vec<_>, String>>()?.into_iter().unzip();
        debug_assert!(pcs.windows(2).all(|pair| pair[0] < pair[1]), "stubs are in PC order");
        Ok(Self { thunk: offset_word(thunk)?, pcs: pcs.into(), offsets: offsets.into() })
    }

    /// The table of a region with no stubs (Cranelift code).
    pub(crate) fn empty() -> Self {
        Self { thunk: 0, pcs: Box::default(), offsets: Box::default() }
    }

    /// The code offset of the resume thunk.
    pub(crate) fn thunk(&self) -> usize {
        self.thunk as usize
    }

    /// The code offset of the stub that re-enters at `pc`.
    pub(crate) fn stub(&self, pc: u32) -> Option<usize> {
        self.pcs.binary_search(&pc).ok().map(|at| self.offsets[at] as usize)
    }

    /// The PCs a stub re-enters at, ascending.
    #[cfg(all(test, target_arch = "x86_64"))]
    pub(crate) fn pcs(&self) -> &[u32] {
        &self.pcs
    }
}

/// The entry rows of one PC's resume stub.
pub(crate) struct Stub {
    pub(crate) pc: u32,
    rows: Vec<EntryRow>,
    /// The row the stub's jump lands on: the PC's first row.
    target: usize,
}

impl Stub {
    /// The hole values of the stub's rows, its jump last.
    pub(crate) fn holes(&self) -> impl Iterator<Item = [u64; HOLES]> + '_ {
        self.rows.iter().map(|row| row.holes).chain([[0; HOLES]])
    }

    /// The exits of the stub's rows, then none for its jump, where a declared test's miss goes to `failed`.
    pub(crate) fn exits(&self, failed: usize) -> impl Iterator<Item = Vec<Target>> + '_ {
        self.rows.iter().map(move |row| row.exits(failed)).chain([Vec::new()])
    }
}

/// A stub's rows in the chain: its first row, its jump and the row the jump lands on.
pub(crate) struct Jump {
    pub(crate) first: usize,
    pub(crate) row: usize,
    pub(crate) target: usize,
}

/// A stub for each caller PC of a loop region that has a row of its own, ascending: not one a tile
/// covers (its first row's instruction is run with it, so the interpreter takes the PC), nor one
/// control never reaches. `first` is the index of the body's first row and `starts` the index of
/// each row's first part among the body's.
pub(crate) fn resume_stubs(layout: &Layout, planned: &[Planned], (first, starts): (usize, &[usize])) -> Result<Vec<Stub>, String> {
    layout
        .entries
        .iter()
        .filter(|&&(_, row)| !planned[row as usize].stencil.code.is_empty() || !planned[row as usize].pre.is_empty())
        // A row a body-established receiver proof reaches stands on the access that proved it, which a resume skips.
        // An entry-seeded proof is re-established by `entry_rows`, which runs every declared guard (`layout.guards`).
        .filter(|&&(_, row)| !layout.relies.get(row as usize).copied().unwrap_or(false))
        .map(|&(pc, row)| {
            let rows = entry_rows(layout, &layout.webs.live_at(&layout.span, row), row as usize)?;
            Ok(Stub { pc, rows, target: first + starts[row as usize] })
        })
        .collect()
}

/// Appends the rows of `stubs` to `rows`, each failed guard taking row `failed` (the failed
/// entry's leave) and each stub ending in `jump`; the rows' holes are `holes` from the first
/// row appended.
pub(crate) fn place_stubs<'a>(rows: &mut Vec<Placed<'a>>, stubs: &'a [Stub], jump: &'a Stencil, holes: &'a [[u64; HOLES]], failed: usize) -> Vec<Jump> {
    let mut hole = rows.len();
    let mut jumps = Vec::with_capacity(stubs.len());
    for stub in stubs {
        let first = rows.len();
        for row in &stub.rows {
            rows.push(Placed { stencil: row.stencil, hole: &holes[hole], taken: row.taken(failed), cold: false });
            hole += 1;
        }
        rows.push(Placed { stencil: jump, hole: &holes[hole], taken: None, cold: false });
        hole += 1;
        jumps.push(Jump { first, row: rows.len() - 1, target: stub.target });
    }
    jumps
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table finds each stub by PC and nothing between or outside them.
    #[test]
    fn the_table_finds_each_stub_by_pc() {
        let table = ResumeTable { thunk: 90, pcs: [4, 5, 9].into(), offsets: [10, 30, 50].into() };
        assert_eq!([4, 5, 9].map(|pc| table.stub(pc)), [Some(10), Some(30), Some(50)]);
        assert_eq!([3, 6, 8, 10].map(|pc| table.stub(pc)), [None; 4]);
        assert_eq!((table.thunk(), ResumeTable::empty().stub(0)), (90, None));
    }

    #[cfg(all(test, target_arch = "x86_64"))]
    fn loop_region(source: &str, name: &str) -> (quench_runtime::bytecode::ResidualProgram, usize, LoopRegion) {
        let program = quench_runtime::Engine::specialize(source, "resume.js").unwrap();
        let id = program.functions.iter().position(|body| body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let function = program.function(id);
        let decoded: Vec<_> = function.instructions().collect();
        let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
            (instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget && (instruction.jump_target() as usize) <= pc)
                .then(|| (instruction.jump_target(), pc as u32))
        }).expect("the function has a loop");
        let region = crate::compiled::osr_loop::region(function, &decoded, &[], &[], None, header, backedge, crate::compiled::osr_loop::MethodCalls::Inliner, &|_| false).expect("the loop is admitted");
        (program, id, region)
    }

    /// A region's code has a stub at each of its PCs a row of its own runs, in order, inside the code
    /// and behind its rows, and its resume thunk is the `entry_resume` routine at the end of the code.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_built_region_has_a_stub_at_each_pc() {
        let (program, id, region) = loop_region("function f(n){let s=0;for(let i=0;i<n;i++){s=s+i;}return s;} f(4);", "f");
        let known = Known { program: &program, stitch: &crate::compiled::tier_policy::STITCH, stores: None, reads: None, adds: None, invalidation: None, constants: &[], typeof_names: &[], presences: None, fields: None, globals: None, classes: &[], shapes: &[] };
        let spec = function::BodySpec { function: id as u32, registers: program.function(id).registers, known, realm: Some(RealmProof::default()), seen: &[], facts: None, typed: &[] };
        let helpers = crate::compiled::process_lane_helpers();
        // SAFETY: the region has no callees, so no entry cell is read.
        let built = unsafe { function::build_region(&region, &spec, ElementViews::Dense, &callees::Callees::default(), helpers) }.expect("the loop has a row for each instruction");
        let (code, table) = (&built.code, &built.resumes);
        assert!(table.pcs().first() == Some(&region.header) && table.pcs().windows(2).all(|pair| pair[0] < pair[1]));
        assert!(table.pcs().iter().all(|pc| (region.header..=region.backedge).contains(pc)));
        let offsets: Vec<usize> = table.pcs().iter().map(|pc| table.stub(*pc).unwrap()).collect();
        assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]), "stubs are laid out in PC order");
        let thunk = &routine("entry_resume").unwrap().code;
        assert_eq!(&code[table.thunk()..], thunk.as_slice(), "the resume thunk ends the code");
        assert!(offsets.iter().all(|offset| *offset > thunk_len() && *offset < table.thunk()));
        assert_eq!(table.stub(region.backedge + 1), None);
    }

    #[cfg(all(test, target_arch = "x86_64"))]
    fn thunk_len() -> usize {
        thunk(Ending::Region, 0, 0).unwrap().len()
    }

    /// A loop whose operands change type mid-run leaves compiled code at a guard and resumes
    /// there; every tier prints what the interpreter prints.
    #[test]
    fn a_loop_that_leaves_mid_iteration_resumes_with_the_interpreter_results() {
        let source = "function run(n, x) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * x) | 0; if (i % 97 === 96) x = x + 0.25; } return s; }
            let total = 0; for (let k = 0; k < 400; k++) total = (total + run(200 + (k & 7), 3)) | 0;
            print(total); print(run(5000, 7)); print(run(300, 2.5)); print(run(300, '3'));";
        let program = quench_runtime::Engine::specialize(source, "resume-differential.js").unwrap();
        let output = |compiled: bool| {
            let lines = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(lines.clone()));
            if compiled {
                vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
                vm.programs.enable_js_lane_methods();
                vm.programs.disable_js_entry_specializations();
            }
            quench_runtime::test_support::execute(&mut vm, &program).unwrap();
            let lines = lines.borrow().clone();
            lines
        };
        assert_eq!(output(true), output(false));
    }
}
