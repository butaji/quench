//! The constants the staged tests share: the word a host callout answers, the element rows
//! under test and the facts of a ten-register tier.

use super::{Operand, Operation};
use super::forms::CallFacts;

/// The word a host callout answers.
pub(super) const ANSWER: u64 = 0xA5A5;
pub(super) const READ: Operation<u64, ()> = Operation::ElementRead { base: 0, key: Operand(1) };
pub(super) const WRITE: Operation<u64, ()> = Operation::ElementWrite { value: 2, base: 0, key: 1 };
pub(super) const FACTS: CallFacts = CallFacts { registers: 10, activates: true, forwarding: false, forms: false };
