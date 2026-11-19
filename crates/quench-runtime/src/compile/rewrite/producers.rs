//! The producers `coalesce_distant_producer_moves` may retarget beyond the adjacent
//! `ProducerMove` recipe.
use super::*;

/// Whether `op` may compute its result straight into a later `Move`'s destination when
/// nothing between them touches either register (`coalesce_distant_producer_moves`).
///
/// Each of these ops writes one result register, named by its A field, and only once its
/// operands are read and its work is done: a throw leaves the result unwritten, so writing a
/// different register is unobservable wherever the register is not read before the `Move`.
/// The distant pass also refuses a producer that reads the source or the destination, so no
/// op here ever reads the register it writes. The adjacent recipe keeps its own list, as it
/// lets a producer read its new destination (`x = x + 1`), which needs each handler to read
/// its operands before writing; this list needs no such property.
///
/// Calls and closure creation stay out: a call's result may share its argument window, and
/// a closure's creation is a binding site's anchor under `with`. The ops listed are the value
/// producers a `let`/`const` initializer, an argument or an assignment commonly ends in:
/// `const v = a[i]`, `let n = +s`, `const o = {}` and `const xs = [a, b]`.
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
                | Op::LoadLocalPlain
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
