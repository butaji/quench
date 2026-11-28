use super::*;

impl FactTables {
    /// What `vm` answers now to the questions `function`'s lowering of `unit` may ask.
    pub(crate) fn of(vm: &Vm, function: u32, unit: &lane_unit::Unit, fields: &[Option<Vec<NativeFieldLoad>>],
        global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> Self {
        use quench_runtime::vm::global_facts::GlobalRead;
        let mut tables = Self::default();
        let read_values = (0..unit.instructions.len()).filter_map(|pc| match global_reads.get(&pc)? {
            GlobalRead::NativeMember { value, .. } => Some(*value),
            read => read.folded_value(),
        });
        let frame_values = unit.frames.iter().filter_map(|frame| frame.target.identity());
        for value in read_values.chain(frame_values) {
            if tables.instanceofs.contains_key(&value.to_bits()) { continue; }
            if let Some(fact) = vm.instanceof_fact(value) { tables.instanceofs.insert(value.to_bits(), fact); }
        }
        for load in fields.iter().flatten().flatten() {
            for shape in std::iter::once(load.shape).chain(load.holder) {
                if vm.shape_is_stable(shape) { tables.stable_shapes.insert(shape); }
            }
        }
        for instruction in &unit.instructions {
            if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::SetFunctionName) {
                let key = (instruction.cache_site_index(), instruction.atom_index());
                if let Some(name) = vm.native_function_name(key.0, key.1) { tables.names.insert(key, name); }
            }
        }
        tables.root_program = vm.active_program;
        tables.root_constants = root_constants(vm, function, unit);
        tables
    }

    pub(crate) fn instanceof(&self, constructor: Value) -> Option<quench_runtime::code_dependencies::InstanceofFact> {
        self.instanceofs.get(&constructor.to_bits()).copied()
    }

    pub(crate) fn shape_stable(&self, shape: u32) -> bool {
        self.stable_shapes.contains(&shape)
    }

    pub(crate) fn name(&self, site: u16, atom: Atom) -> Option<(quench_runtime::staged::add_plan::Transition, u64)> {
        self.names.get(&(site, atom)).copied()
    }

    /// The root constant the capture read at `pc` takes (`root_constants`), with its program.
    pub(crate) fn root_constant(&self, pc: usize) -> Option<(quench_runtime::vm::program_store::ProgramId, Value)> {
        self.root_constants.get(&pc).map(|value| (self.root_program, *value))
    }
}

/// The program a lowering reads: the running one, on the program's thread, or a copy of it that
/// shares nothing with it (`ResidualProgram::detached`), on the code generator's.
pub(crate) enum LoweringProgram<'p> {
    #[cfg_attr(not(test), allow(dead_code))]
    Running(&'p ResidualProgram),
    Detached(quench_runtime::bytecode::DetachedProgram),
}

impl std::ops::Deref for LoweringProgram<'_> {
    type Target = ResidualProgram;
    fn deref(&self) -> &ResidualProgram {
        match self {
            Self::Running(program) => program,
            Self::Detached(program) => program,
        }
    }
}

