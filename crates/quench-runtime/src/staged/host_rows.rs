//! The rows a host operation declares (`Host::function_row`): the case of its calls a staged
//! row decides before the host's own steps run. Every tier runs the row from here, so a
//! specializer that proves a call site's arguments takes the declared operation alone, and an
//! image links only it, not the steps the row leaves to.

use super::Stage;
use super::value::is_direct_string;
use crate::Value;

/// A row a host operation may declare.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostRow {
    /// A text writer: a call whose arguments past the first `bound` (the ones a bound method
    /// passes) are all strings is first a call of host operation `plain`, with the same
    /// arguments, which writes them as text and answers `undefined`, or answers `false` for a
    /// call it leaves to the operation's own steps; any other call runs those steps.
    PlainText { bound: u8, plain: u32 },
}

/// Admits `HostRow::PlainText`: every argument word past the first `bound` is a direct string
/// reference (the one encoding of a string), else leaves at `miss`.
#[inline(always)]
pub fn admit_plain_text<S: Stage>(stage: &mut S, arguments: &[S::Word], bound: usize, miss: S::Miss) -> Option<()> {
    for &argument in arguments.iter().skip(bound) {
        let string = is_direct_string(stage, argument);
        stage.guard(string, miss)?;
    }
    Some(())
}

/// The host operation a call declaring `row` makes first with `arguments`, when the row admits
/// them: it answers `false` for a call it leaves to the declaring operation's own steps.
#[inline]
pub fn concrete_plain(row: Option<HostRow>, arguments: &[Value]) -> Option<u32> {
    let Some(HostRow::PlainText { bound, plain }) = row else { return None };
    let words: smallvec::SmallVec<[u64; 8]> = arguments.iter().map(|value| value.to_bits()).collect();
    admit_plain_text(&mut super::Concrete::pure(), &words, usize::from(bound), ()).map(|()| plain)
}
