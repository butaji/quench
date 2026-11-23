//! A copy of a program that shares no reference-counted part with it (`DetachedProgram`), so a
//! lane body's lowering may read it on the code generator's thread while the program runs on.
//! The lowering reads a program's functions, constants, atoms, field sites and
//! superinstructions (`lane_region`); every one of those is copied here, deep where the
//! original shares a count (`Rc`): a string constant's text, the atom table's text, a function's
//! root masks. A function the copy is not asked for keeps only its header.

use super::*;

/// A program copy whose parts no other value counts: `Send`, though `ResidualProgram` is not.
pub struct DetachedProgram(ResidualProgram);

// SAFETY: `ResidualProgram::detached` builds it of owned parts only: its `Rc` fields are
// `None`, its constants, atoms and functions are deep copies whose counted parts (string
// texts, atom texts, root masks) are fresh allocations of their own, and name bindings, the
// one counted part of a function the lowering does not read, are left empty. No count it
// holds is shared with a value of another thread, so moving it to one is sound; it is used by
// one thread at a time.
unsafe impl Send for DetachedProgram {}

impl std::ops::Deref for DetachedProgram {
    type Target = ResidualProgram;
    fn deref(&self) -> &ResidualProgram {
        &self.0
    }
}

impl Constant {
    /// The constant with a text of its own.
    fn detached(&self) -> Self {
        match self {
            Self::String(text) => Self::String(std::rc::Rc::from(&**text)),
            other => other.clone(),
        }
    }
}

impl Function {
    /// The function with its counted parts copied: its name bindings empty. Its register roots
    /// hold no shared allocation (an entry bound only), so the clone copies them.
    fn detached(&self) -> Self {
        let mut function = self.clone();
        function.name_bindings = NameBindings::default();
        function
    }
}

impl ResidualProgram {
    /// A copy of the program for a lowering that reads the functions `functions` (and any
    /// header): see `DetachedProgram`.
    pub fn detached(&self, functions: &[u32]) -> DetachedProgram {
        let names: Vec<std::rc::Rc<str>> = self.atoms.iter().map(std::rc::Rc::from).collect();
        let slots: Vec<FunctionSlot> = (0..self.functions.len()).map(|index| {
            let slot = &self.functions[index];
            match functions.contains(&(index as u32)).then(|| self.function_get(index)).flatten() {
                Some(function) => FunctionSlot::compiled(function.detached()),
                None => FunctionSlot::released(slot.header()),
            }
        }).collect();
        DetachedProgram(ResidualProgram {
            specialized: self.specialized,
            kind: self.kind,
            module_requests: Vec::new(),
            module_imports: Vec::new(),
            module_link_plan: None,
            declared_types: None,
            source_name: String::new(),
            source: None,
            atoms: AtomTable::from_rcs(&names),
            constants: self.constants.iter().map(Constant::detached).collect::<Vec<_>>().into(),
            functions: slots.into(),
            cache_sites: self.cache_sites.clone(),
            method_sites: self.method_sites.iter().cloned().collect::<Vec<_>>().into(),
            method_arguments: self.method_arguments.iter().copied().collect::<Vec<_>>().into(),
            field_sites: self.field_sites.iter().cloned().collect::<Vec<_>>().into(),
            object_sites: self.object_sites.iter().cloned().collect::<Vec<_>>().into(),
            regexp_literal_sites: Vec::new().into(),
            literal_templates: self.literal_templates.iter().cloned().collect::<Vec<_>>().into(),
            superinstructions: self.superinstructions.iter().cloned().collect::<Vec<_>>().into(),
            rewrite_ranges: Default::default(),
            replay: None,
            first_call_scopes: Default::default(),
            encoded_records: None,
            validated: Default::default(),
            undefined_constant: Default::default(),
            closure_reads: None,
            wasm_lazy: None,
            load_encoding: Default::default(),
        })
    }
}
