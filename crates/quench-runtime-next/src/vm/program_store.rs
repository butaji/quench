use crate::Value;
use crate::bytecode::ResidualProgram;
use std::rc::Rc;

struct ProgramEntry {
    residual: Rc<ResidualProgram>,
    constants: Vec<Value>,
    const_arrays: Vec<Option<Rc<Vec<Value>>>>,
    module_environment: Option<Value>,
    import_meta: Option<Value>,
    module_imports: Vec<(u16, ModuleImport)>,
    module: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ModuleImport {
    Value(Value),
    Binding(ProgramId, u16),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ProgramId(u32);

impl ProgramId {
    pub(crate) const MAIN: Self = Self(0);

    fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Default)]
pub(crate) struct ProgramStore {
    programs: Vec<ProgramEntry>,
}

impl ProgramStore {
    pub(crate) fn len(&self) -> usize {
        self.programs.len()
    }

    pub(crate) fn reset(&mut self, main: &ResidualProgram) -> ProgramId {
        self.programs.clear();
        let module = main.module;
        let id = self.insert(main.clone()).unwrap_or(ProgramId::MAIN);
        if module && let Some(entry) = self.programs.get_mut(id.index()) {
            entry.module = true;
        }
        id
    }

    pub(crate) fn insert(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let id = ProgramId::from_index(self.programs.len())?;
        self.programs.push(ProgramEntry {
            residual: Rc::new(program),
            constants: Vec::new(),
            const_arrays: Vec::new(),
            module_environment: None,
            import_meta: None,
            module_imports: Vec::new(),
            module: false,
        });
        Some(id)
    }

    pub(crate) fn insert_module(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let id = self.insert(program)?;
        self.programs[id.index()].module = true;
        Some(id)
    }

    pub(crate) fn is_module(&self, id: ProgramId) -> bool {
        self.programs
            .get(id.index())
            .is_some_and(|entry| entry.module)
    }

    pub(crate) fn set_module_environment(&mut self, id: ProgramId, environment: Value) {
        if let Some(entry) = self.programs.get_mut(id.index())
            && entry.module
        {
            entry.module_environment = Some(environment);
        }
    }

    pub(crate) fn module_environment(&self, id: ProgramId) -> Option<Value> {
        self.programs.get(id.index())?.module_environment
    }

    pub(crate) fn import_meta(&self, id: ProgramId) -> Option<Value> {
        self.programs.get(id.index())?.import_meta
    }

    pub(crate) fn set_import_meta(&mut self, id: ProgramId, value: Value) {
        if let Some(entry) = self.programs.get_mut(id.index())
            && entry.module
        {
            entry.import_meta = Some(value);
        }
    }

    pub(crate) fn set_module_imports(&mut self, id: ProgramId, imports: Vec<(u16, ModuleImport)>) {
        if let Some(entry) = self.programs.get_mut(id.index()) {
            entry.module_imports = imports;
        }
    }

    pub(crate) fn module_import(&self, id: ProgramId, slot: u16) -> Option<ModuleImport> {
        self.programs.get(id.index()).and_then(|entry| {
            entry
                .module_imports
                .iter()
                .find_map(|(local, import)| (*local == slot).then_some(*import))
        })
    }

    pub(crate) fn get(&self, id: ProgramId) -> Option<Rc<ResidualProgram>> {
        self.programs
            .get(id.index())
            .map(|entry| entry.residual.clone())
    }

    pub(crate) fn set_constants(&mut self, id: ProgramId, constants: Vec<Value>) {
        if let Some(entry) = self.programs.get_mut(id.index()) {
            entry.const_arrays = vec![None; constants.len()];
            entry.constants = constants;
        }
    }

    pub(crate) fn constant(&self, id: ProgramId, index: usize) -> Option<Value> {
        self.programs.get(id.index())?.constants.get(index).copied()
    }

    pub(crate) fn const_array(
        &mut self,
        id: ProgramId,
        start: usize,
        count: usize,
    ) -> Option<Rc<Vec<Value>>> {
        let entry = self.programs.get_mut(id.index())?;
        let end = start.checked_add(count)?;
        let cached = entry.const_arrays.get_mut(start)?;
        if cached.is_none() {
            *cached = Some(Rc::new(entry.constants.get(start..end)?.to_vec()));
        }
        cached.clone()
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.programs.iter().flat_map(|entry| {
            entry
                .constants
                .iter()
                .copied()
                .chain(entry.module_environment)
                .chain(entry.import_meta)
                .chain(
                    entry
                        .module_imports
                        .iter()
                        .filter_map(|(_, import)| match import {
                            ModuleImport::Value(value) => Some(*value),
                            ModuleImport::Binding(_, _) => None,
                        }),
                )
        })
    }
}
