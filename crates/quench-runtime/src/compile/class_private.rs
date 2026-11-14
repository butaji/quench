use super::*;

impl FunctionCompiler<'_, '_> {
    /// The private names `class` declares this evaluation, each with the home atom that owns it (the class itself
    /// for a static name, the constructor's home for an instance one), in source order; a name an inner class
    /// overrides is left to it.
    pub(super) fn class_private_bindings(&mut self, class: &Class<'_>, (class_home_atom, constructor_home_atom): (Atom, Atom)) -> Vec<(Atom, Atom)> {
        let mut private_bindings = Vec::new();
        for element in &class.body.body {
            if let ClassElement::AccessorProperty(accessor) = element {
                let atom = self.owner.private_name_atom(accessor.span);
                let home = if accessor.r#static {
                    class_home_atom
                } else {
                    constructor_home_atom
                };
                private_bindings.push((atom, home));
            }
            let (key, is_static) = match element {
                ClassElement::PropertyDefinition(field) => (&field.key, field.r#static),
                ClassElement::MethodDefinition(method) => (&method.key, method.r#static),
                ClassElement::AccessorProperty(accessor) => (&accessor.key, accessor.r#static),
                _ => continue,
            };
            if let PropertyKey::PrivateIdentifier(identifier) = key {
                let atom = self.owner.private_name_atom(identifier.span);
                if !self.owner.private_name_is_overridden(atom) {
                    private_bindings.push((atom, if is_static { class_home_atom } else { constructor_home_atom }));
                }
            }
        }
        private_bindings
    }
}
