//! Tag and shape tests the solver type already decides. A register whose solver type is a subtype of a tag's type holds that
//! tag at every point it reaches, so a site that would test the tag emits no test for it (the known-types law): a
//! test is made only where the type cannot prove the tag. A direct object's solver type proves its family (a non-nullable
//! object record, never a function), so the family and presence tests on it are redundant too.
use super::*;

/// Whether register `register` holds `tag`'s language types at this point, by its solver type in `state`. A tag site
/// asks this before it emits its tag test.
pub(crate) fn register_proves(state: &State, register: Register, tag: Type) -> bool {
    state.values[usize::from(register)].value.ty.is_subtype(tag)
}

/// Whether a value of solver type `ty` is a direct object's record (`Type::is_direct_object_family`): its record is present
/// and it is not a function, so a test that it is an object, or that its record is present, is redundant.
pub(crate) fn proves_object(ty: Type) -> bool {
    ty.is_direct_object_family()
}
