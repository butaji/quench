//! Resume stubs: where a run that left the region's PC (a Cranelift body's guard, say) re-enters
//! its stencil code. A stub is the entry rows for the webs live at its PC (each pinned register
//! loaded and checked from its frame slot, as at the header) and a jump to the PC's first row;
//! the table names each stub's code offset, and the resume thunk (`entry_resume`) calls it
//! through the shared entry ABI, so a register that holds no Number there leaves with
//! `LaneExit::Entry` and the interpreter resumes at the PC.

use super::*;
use quench_bjit::HOLES;

/// Where a region's code can be re-entered: the resume thunk and, sorted by PC, each stub.
pub(in crate::vm) struct ResumeTable {
    thunk: u32,
    pcs: Box<[u32]>,
    offsets: Box<[u32]>,
}

/// A code offset as the table keeps it.
fn offset_word(offset: usize) -> Result<u32, String> {
    u32::try_from(offset).map_err(|_| format!("code offset {offset} past the table's reach"))
}

impl ResumeTable {
    /// The table of `stubs`, whose first rows `jumps` name, over the code `offsets` of the rows.
    pub(super) fn of(thunk: usize, stubs: &[Stub], jumps: &[Jump], offsets: &[usize]) -> Result<Self, String> {
        let entries = stubs.iter().zip(jumps).map(|(stub, jump)| Ok((stub.pc, offset_word(offsets[jump.first])?)));
        let (pcs, offsets): (Vec<u32>, Vec<u32>) = entries.collect::<Result<Vec<_>, String>>()?.into_iter().unzip();
        debug_assert!(pcs.windows(2).all(|pair| pair[0] < pair[1]), "stubs are in PC order");
        Ok(Self { thunk: offset_word(thunk)?, pcs: pcs.into(), offsets: offsets.into() })
    }

    /// The table of a region with no stubs (Cranelift code).
    pub(in crate::vm) fn empty() -> Self {
        Self { thunk: 0, pcs: Box::default(), offsets: Box::default() }
    }

    /// The code offset of the resume thunk.
    pub(in crate::vm) fn thunk(&self) -> usize {
        self.thunk as usize
    }

    /// The code offset of the stub that re-enters at `pc`.
    pub(in crate::vm) fn stub(&self, pc: u32) -> Option<usize> {
        self.pcs.binary_search(&pc).ok().map(|at| self.offsets[at] as usize)
    }

    /// The PCs a stub re-enters at, ascending.
    #[cfg(test)]
    pub(in crate::vm) fn pcs(&self) -> &[u32] {
        &self.pcs
    }
}

/// The entry rows of one PC's resume stub.
pub(super) struct Stub {
    pub(super) pc: u32,
    rows: Vec<(&'static Stencil, [u64; HOLES])>,
    /// The row the stub's jump lands on: the PC's first row.
    target: usize,
}

impl Stub {
    /// The hole values of the stub's rows, its jump last.
    pub(super) fn holes(&self) -> impl Iterator<Item = [u64; HOLES]> + '_ {
        self.rows.iter().map(|(_, holes)| *holes).chain([[0; HOLES]])
    }
}

/// A stub's rows in the chain: its first row, its jump and the row the jump lands on.
pub(super) struct Jump {
    pub(super) first: usize,
    pub(super) row: usize,
    pub(super) target: usize,
}

/// A stub for each PC of the region, ascending; `first` is the index of the body's first row
/// and `starts` the index of each PC's first row among the body's.
pub(super) fn resume_stubs(layout: &Layout, first: usize, starts: &[usize]) -> Result<Vec<Stub>, String> {
    let header = layout.header;
    (header..=layout.backedge)
        .map(|pc| {
            let rows = entry_rows(layout, &layout.webs.live_at(layout.region, pc))?;
            Ok(Stub { pc, rows, target: first + starts[(pc - header) as usize] })
        })
        .collect()
}

/// Appends the rows of `stubs` to `rows`, each failed guard taking row `failed` (the failed
/// entry's leave) and each stub ending in `jump`; the rows' holes are `holes` from the first
/// row appended.
pub(super) fn place_stubs<'a>(rows: &mut Vec<Placed<'a>>, stubs: &'a [Stub], jump: &'a Stencil, holes: &'a [Box<dyn Fn(u32) -> u64 + 'a>], failed: usize) -> Vec<Jump> {
    let mut hole = rows.len();
    let mut jumps = Vec::with_capacity(stubs.len());
    for stub in stubs {
        let first = rows.len();
        for (stencil, _) in &stub.rows {
            rows.push(Placed { stencil, hole: &*holes[hole], taken: Some(failed) });
            hole += 1;
        }
        rows.push(Placed { stencil: jump, hole: &*holes[hole], taken: None });
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

    fn loop_region(source: &str, name: &str) -> LoopRegion {
        let program = crate::Engine::specialize(source, "resume.js").unwrap();
        let id = program.functions.iter().position(|body| body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let function = program.function(id);
        let decoded: Vec<_> = function.instructions().collect();
        let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
            (instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget && (instruction.jump_target() as usize) <= pc)
                .then(|| (instruction.jump_target(), pc as u32))
        }).expect("the function has a loop");
        super::super::super::osr_loop::region(function, &decoded, &[], &[], None, header, backedge, super::super::super::osr_loop::MethodCalls::Inliner).expect("the loop is admitted")
    }

    /// A region's code has a stub at each of its PCs, in order, inside the code and behind its
    /// rows, and its resume thunk is the `entry_resume` routine at the end of the code.
    #[test]
    fn a_built_region_has_a_stub_at_each_pc() {
        let region = loop_region("function f(n){let s=0;for(let i=0;i<n;i++){s=s+i;}return s;} f(4);", "f");
        let built = build(&region, true, &[], [&[], &[]], &[], &[]).expect("the loop has a row for each instruction");
        let (code, table) = (&built.code, &built.resumes);
        assert_eq!(table.pcs(), (region.header..=region.backedge).collect::<Vec<_>>());
        let offsets: Vec<usize> = table.pcs().iter().map(|pc| table.stub(*pc).unwrap()).collect();
        assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]), "stubs are laid out in PC order");
        let thunk = routine("entry_resume").unwrap().code;
        assert_eq!(&code[table.thunk()..], thunk.as_slice(), "the resume thunk ends the code");
        assert!(offsets.iter().all(|offset| *offset > thunk_len() && *offset < table.thunk()));
        assert_eq!(table.stub(region.backedge + 1), None);
    }

    fn thunk_len() -> usize {
        thunk(0).unwrap().len()
    }

    /// A loop whose operands change type mid-run leaves compiled code at a guard and resumes
    /// there; every tier prints what the interpreter prints.
    #[test]
    fn a_loop_that_leaves_mid_iteration_resumes_with_the_interpreter_results() {
        let source = "function run(n, x) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * x) | 0; if (i % 97 === 96) x = x + 0.25; } return s; }
            let total = 0; for (let k = 0; k < 400; k++) total = (total + run(200 + (k & 7), 3)) | 0;
            print(total); print(run(5000, 7)); print(run(300, 2.5)); print(run(300, '3'));";
        let program = crate::Engine::specialize(source, "resume-differential.js").unwrap();
        let output = |compiled: bool| {
            let lines = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(lines.clone()));
            if compiled {
                vm.set_execution_mode(crate::ExecutionMode::Compiled);
                vm.programs.enable_js_lane_methods();
                vm.programs.disable_js_entry_specializations();
            }
            vm.execute(&program).unwrap();
            let lines = lines.borrow().clone();
            lines
        };
        assert_eq!(output(true), output(false));
    }
}
