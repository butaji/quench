//! The producers `coalesce_distant_producer_moves` may retarget beyond the adjacent
//! `ProducerMove` recipe.
use super::*;

/// Whether `op` may compute its result straight into a later `Move`'s destination when
/// nothing between them touches either register (`coalesce_distant_producer_moves`).
///
/// Each op added here is no call and writes exactly one register, its A field's result, with
/// no other role: which register receives the value changes nothing else it does. The distant
/// pass refuses a producer that reads the source or the destination, and one whose span a
/// reading handler covers, so no op here reads the register it now writes, and no abrupt
/// completion observes the swap. The adjacent recipe keeps its own list, as it lets a
/// producer read its new destination (`x = x + 1`), which needs each handler to read its
/// operands before writing; this list needs no such property.
///
/// The ops added are the value producers a `let`/`const` initializer, an argument or an
/// assignment commonly ends in: `const v = a[i]`, `let n = +s`, `const o = {}` and
/// `const xs = [1, 2]`. Closure creation stays out, as under `with` it anchors a binding site.
pub(super) fn coalesces_at_a_distance(op: Op) -> bool {
    fuses(op, Op::Move, |recipe| matches!(recipe, Recipe::ProducerMove))
        || matches!(
            op,
            Op::GetIndex
                | Op::ToNumeric
                | Op::ToPropertyKey
                | Op::MakeArray
                | Op::MakeConstArray
                | Op::MakeObject
                | Op::MakeObjectLiteral
                | Op::MakeLiteralTemplate
                | Op::CreateRegExpLiteral
                | Op::LoadCallee
                | Op::LoadNameTypeof
                | Op::LoadCachedTemplateObject
                | Op::StringConcat
                | Op::ArgumentsLength
                | Op::ArgumentGet
                | Op::NumericAdd
                | Op::NumericMultiply
                | Op::PrivateIn
        )
}
