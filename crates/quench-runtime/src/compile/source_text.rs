//! The text a compiled program keeps of its source.

use crate::bytecode::SourceText;

/// The program's copy of `text`, the source it compiled. An eval's is local, so the eval
/// string can view it instead of keeping its own (`Vm::keep_eval_text`): a method eval
/// compiles its text inside a wrapper, which the string's storage does not hold.
pub(super) fn kept(text: &str, eval: bool) -> SourceText {
    if eval {
        SourceText::Local(std::rc::Rc::from(text))
    } else {
        SourceText::Shared(std::sync::Arc::from(text))
    }
}

/// `provided` when it holds exactly `text` (the same storage, or equal bytes), which the program
/// keeps without a copy of its own; otherwise the copy `kept` makes. A wrapped or
/// hashbang-stripped text is never `provided`'s.
pub(super) fn kept_or_provided(text: &str, eval: bool, provided: Option<SourceText>) -> SourceText {
    match provided {
        Some(provided)
            if provided.len() == text.len()
                && (std::ptr::eq(provided.as_ptr(), text.as_ptr()) || *provided == *text) => provided,
        _ => kept(text, eval),
    }
}
