//! The closed-world names of the method caches and of the global object: which atoms no method cache can name
//! (`uncached_atoms`), and which own properties the global object has (`GlobalShape`). Both are edge facts of the
//! closed world: they hold only where `World::Closed` verified, and every other world answers empty or none.

use std::collections::BTreeSet;

use super::flow::FunctionFlow;
use super::gate::World;
use super::quiet::Proofs;
use super::value::Key;
use crate::bytecode::ResidualProgram;

/// The atoms no method cache can name: every atom of the program that no method call site of the program names. A
/// method cache records an atom only when a method call site with that atom executes (`call_method_site_safe`), and
/// the sites are the program's own `method_sites`, which is exact whether or not the flows reach every call. Empty
/// in any world but `World::Closed`, where new code may add method call sites. Another program already loaded into
/// the VM may have named an atom too, so a consumer also checks the VM's caches at compile time and subscribes to
/// their readers (`MethodCacheAtoms::readers`), which retire the code when a cache names the atom later.
pub(super) fn uncached_atoms(program: &ResidualProgram, world: World) -> BTreeSet<u32> {
    if world != World::Closed {
        return BTreeSet::new();
    }
    let named: BTreeSet<u32> = program.method_sites.iter().map(|site| site.atom).collect();
    program.atoms.iter().enumerate().map(|(atom, _)| atom as u32).filter(|atom| !named.contains(atom)).collect()
}

/// The own properties of the global object the closed world fixes (`AotFacts::global_shape`): the names the program
/// declares, and the builtins it reads that no code of the program changes. A name in neither is no claim.
///
/// The realm decides a builtin's presence and its slot, so a builtin named here is fixed as a name, not proven present:
/// a read of it still takes the realm's own slot (`Vm::stable_global_slot`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalShape {
    /// The global names the program writes (`var`, function and assignment to an undeclared name), and the properties it
    /// stores on the global object where a store may reach it (`global_source`). Each is an own property from its first write.
    declared: BTreeSet<u32>,
    /// The global names the program reads and does not declare: the host's builtins, which no store, define or delete of
    /// the program changes (`Proofs::global_object_fixed`).
    builtins: BTreeSet<u32>,
}

impl GlobalShape {
    /// The global names the program declares (its own properties from their first write).
    pub fn declared(&self) -> &BTreeSet<u32> {
        &self.declared
    }

    /// The host builtins the program reads by name and never declares. Presence is the realm's, not the program's.
    pub fn builtins(&self) -> &BTreeSet<u32> {
        &self.builtins
    }

    /// The names the closed world's scripts declare with `let`, `const` or `class` at top level (`Function::global_lexical_atoms`
    /// of every function that declares one), sorted and deduped, as `AotFacts::global_lexical_names` reads them. Empty in any
    /// world but `World::Closed`: a script of an open world may declare a name later, so no name is fixed.
    pub(super) fn global_lexical_names(program: &ResidualProgram, world: World) -> Vec<u32> {
        if world != World::Closed {
            return Vec::new();
        }
        let names: BTreeSet<u32> = (0..program.function_count())
            .flat_map(|id| program.with_record(id, |function| function.global_lexical_atoms().to_vec()))
            .collect();
        names.into_iter().collect()
    }

    /// The shape of the global object as the closed world fixes it, `None` in any world but `World::Closed`, and where
    /// `Proofs::global_object_fixed` fails: a store, define or delete reaches the global object by a name the analysis
    /// cannot follow, or a store through a computed key may write a global name while the global object is a value.
    pub(super) fn of(flows: &[FunctionFlow], world: World) -> Option<Self> {
        if world != World::Closed {
            return None;
        }
        let proofs = Proofs::of(flows);
        if !proofs.global_object_fixed() {
            return None;
        }
        let mut declared = proofs.written.clone();
        if proofs.global_source {
            declared.extend(proofs.stored.iter().copied());
        }
        let builtins = flows
            .iter()
            .flat_map(|flow| flow.reads.iter())
            .filter_map(|key| match key {
                Key::Global(atom) => Some(*atom),
                _ => None,
            })
            .filter(|atom| !declared.contains(atom))
            .collect();
        Some(Self { declared, builtins })
    }
}
