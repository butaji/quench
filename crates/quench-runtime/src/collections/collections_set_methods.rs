fn is_set_relation(builtin: crate::ops::Builtin) -> bool {
    matches!(
        builtin,
        crate::ops::Builtin::SetDifference
            | crate::ops::Builtin::SetIntersection
            | crate::ops::Builtin::SetSymmetricDifference
            | crate::ops::Builtin::SetUnion
            | crate::ops::Builtin::SetIsDisjointFrom
            | crate::ops::Builtin::SetIsSubsetOf
            | crate::ops::Builtin::SetIsSupersetOf
    )
}
