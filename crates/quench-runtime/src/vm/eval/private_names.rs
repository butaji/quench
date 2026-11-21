//! The private names a direct eval sees: those the calling function's scopes declare, with the home
//! object whose private declarations they name.
use super::*;
use crate::bytecode::private_identity_label;

/// Whether `binding` declares a private name.
fn is_private_name(binding: &crate::bytecode::EvalBinding) -> bool {
    binding.kind == crate::bytecode::LexicalBindingKind::PrivateName
}

impl Vm {
    /// The home object and the `(label, identity)` of each private name the running frame's direct
    /// eval sees, those its scopes declare first, else the home binding's own declarations.
    pub(super) fn direct_eval_private_names(&mut self, p: &ResidualProgram) -> Option<(Value, Vec<(String, String)>)> {
        let frame = self.frames.len().checked_sub(1)?;
        let caller = self.frame_caller(frame);
        let mut atoms = Vec::new();
        let metadata = p.function_get(caller.function() as usize)?;
        if let Some(pc) = caller.binding_site()
            && let Ok(index) = metadata.binding_sites.binary_search_by_key(&pc, |site| site.resume_pc)
        {
            atoms.extend(metadata.binding_sites[index].bindings.iter().filter(|binding| is_private_name(binding)).map(|binding| binding.atom));
        }
        atoms.extend(metadata.name_bindings.resolved().into_iter().filter(is_private_name).map(|binding| binding.atom));
        self.enclosing_private_atoms(self.captured_parent_environment(caller), &mut atoms);
        atoms.sort_unstable();
        atoms.dedup();
        let mut bindings = atoms.into_iter().filter_map(|atom| self.name_binding(caller, atom)).filter(is_private_name).collect::<Vec<_>>();
        bindings.sort_by_key(|binding| match binding.location { crate::bytecode::EvalBindingLocation::Local(_) => 0, crate::bytecode::EvalBindingLocation::Capture { depth, .. } => u32::from(depth)+1 });
        let mut names = Vec::new();
        let mut first_home = None;
        for binding in bindings {
            let identity = self.atom_name(binding.atom).to_owned();
            let Some(label) = private_identity_label(&identity).map(str::to_owned) else { continue; };
            if names.iter().any(|(name, _)| name == &label) { continue; }
            if let Ok(home) = self.load_name_binding(p, VarOwner::Frame(frame), binding) {
                first_home.get_or_insert(home);
                names.push((label, identity));
            }
        }
        if !names.is_empty() { return Some((first_home?, names)); }
        self.home_private_names(p)
    }

    /// Adds to `atoms` the private names the functions of `environment`'s chain declare.
    fn enclosing_private_atoms(&self, mut environment: Value, atoms: &mut Vec<Atom>) {
        while let Some(crate::heap::EnvironmentRecord { parent, function, scope, .. }) = self.heap.environment(environment) {
            if let Some(owner) = scope.program().and_then(|program| self.programs.get(ProgramId::from_raw(program)))
                && let Some(metadata) = owner.function_get(*function as usize)
            {
                atoms.extend(metadata.name_bindings.resolved().into_iter().filter(is_private_name).map(|binding| binding.atom));
            }
            environment = *parent;
        }
    }

    /// The home binding and the labels of the private names its object declares.
    fn home_private_names(&mut self, p: &ResidualProgram) -> Option<(Value, Vec<(String, String)>)> {
        let home = self.eval_home_binding(p)?;
        let names = self.private_names(home).into_iter().filter_map(|brand| {
            let identity = self.atom_name(brand.name).to_owned();
            private_identity_label(&identity).map(str::to_owned).map(|label| (label, identity))
        }).fold(Vec::new(), |mut names, binding| { if !names.iter().any(|(label, _)| label == &binding.0) { names.push(binding); } names });
        (!names.is_empty()).then_some((home, names))
    }
}
