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
