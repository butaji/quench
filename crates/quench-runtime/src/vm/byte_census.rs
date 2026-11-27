//! `QUENCH_DEV=byte-census`: the bytes of each category at exit, with its role. Truth is what
//! the run-time input forces to exist; cache is what can be dropped and rebuilt.

use super::Vm;
use crate::bytecode::{Function, FunctionSlot, Instr, WideInstruction};
use crate::trace::Dev;

#[derive(Clone, Copy)]
enum Role {
    Truth,
    Cache,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::Truth => "truth",
            Role::Cache => "cache",
        }
    }
}

/// The categories in dump order, each with its role.
const CATEGORIES: [(&str, Role); 6] = [
    ("bytecode", Role::Truth),
    ("facts", Role::Truth),
    ("live_heap", Role::Truth),
    ("code", Role::Cache),
    ("metadata", Role::Cache),
    ("profiling", Role::Cache),
];

impl Vm {
    pub(crate) fn report_byte_census(&self) {
        if !Dev::ByteCensus.on() {
            return;
        }
        let composition = self.heap.memory_composition();
        let bytes = [
            Some(self.bytecode_bytes()),
            Some(self.heap.dependency_bytes() + self.method_cache_atoms.reader_bytes()),
            Some(composition.used_bytes()),
            Some(self.programs.jit_mapped_bytes()),
            Some(
                self.shapes.capacity() * size_of::<super::Shape>()
                    + self.field_caches.capacity_bytes()
                    + self.method_caches.capacity_bytes(),
            ),
            cfg!(feature = "profile-memory").then_some(composition.profile_instrumentation_bytes),
        ];
        let categories = CATEGORIES.iter().zip(bytes).map(|((name, role), bytes)| {
            let bytes = bytes.map_or_else(|| "null".to_string(), |bytes| bytes.to_string());
            format!(
                "\"{name}\":{{\"role\":\"{}\",\"bytes\":{bytes}}}",
                role.label()
            )
        });
        let categories = categories.collect::<Vec<_>>().join(",");
        eprintln!("QUENCH_BYTE_CENSUS {{{categories},\"facts_unread_bytes\":null}}");
    }

    /// The records and code of every function of every loaded program.
    fn bytecode_bytes(&self) -> usize {
        self.programs
            .residuals()
            .flat_map(|program| program.functions.iter().filter_map(FunctionSlot::record))
            .map(|function| {
                size_of::<Function>()
                    + function.code.capacity() * size_of::<Instr>()
                    + function.wide.capacity() * size_of::<WideInstruction>()
            })
            .sum()
    }
}
