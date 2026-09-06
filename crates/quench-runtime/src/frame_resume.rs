impl Frame {
    pub(crate) fn set_catch_resume(&mut self, range: CodeRange, yield_dst: u16) -> bool {
        let Self::Try {
            phase,
            body_resume,
            yield_dst: dst,
            ..
        } = self
        else {
            return false;
        };
        set_resume(phase, body_resume, dst, TryPhase::Catch, range, yield_dst);
        true
    }

    pub(crate) fn set_finally_resume(&mut self, range: CodeRange, yield_dst: u16) -> bool {
        let Self::Try {
            phase,
            body_resume,
            yield_dst: dst,
            ..
        } = self
        else {
            return false;
        };
        set_resume(phase, body_resume, dst, TryPhase::Finally, range, yield_dst);
        true
    }

    fn advance_resume(&mut self, range: CodeRange, yield_dst: u16) -> bool {
        match self {
            Self::Try {
                phase,
                body_resume,
                yield_dst: dst,
                ..
            } => {
                let next_phase = if matches!(phase, TryPhase::Finally) {
                    TryPhase::Finally
                } else {
                    TryPhase::Body
                };
                set_resume(phase, body_resume, dst, next_phase, range, yield_dst)
            }
            Self::Iterator {
                phase,
                body_resume,
                yield_dst: dst,
                ..
            } => set_resume(
                phase,
                body_resume,
                dst,
                IteratorPhase::Body,
                range,
                yield_dst,
            ),
            Self::Branch {
                phase,
                branch_resume,
                yield_dst: dst,
                ..
            } => set_resume(
                phase,
                branch_resume,
                dst,
                BranchPhase::Body,
                range,
                yield_dst,
            ),
            Self::Private {
                phase,
                body_resume,
                yield_dst: dst,
                ..
            } => set_resume(
                phase,
                body_resume,
                dst,
                PrivatePhase::Body,
                range,
                yield_dst,
            ),
            Self::Loop { .. } => return false,
            _ => return false,
        }
        true
    }
    /// Every executable continuation must point into the machine's immutable
    /// code store; frames never own or re-walk AST bodies.
    pub(crate) fn ranges(&self) -> Vec<CodeRange> {
        match self {
            Self::Try { body, handler, finalizer, body_resume, resume, .. } => {
                let mut out = vec![*body, *body_resume, *resume];
                out.extend(handler.iter().copied());
                out.extend(finalizer.iter().copied());
                out
            }
            Self::Iterator { body, body_resume, resume, .. } => vec![*body, *body_resume, *resume],
            Self::Branch { branch_resume, resume, .. } => vec![*branch_resume, *resume],
            Self::Private { body_resume, resume, .. } => vec![*body_resume, *resume],
            Self::Loop { body, test, update, phase_resume, resume, .. } => {
                vec![*body, *test, *update, *phase_resume, *resume]
            }
            Self::Await { resume, .. } => vec![*resume],
            Self::Delegate { .. } => Vec::new(),
            Self::Dispose { body_resume, resume, .. } => vec![*body_resume, *resume],
        }
    }
    /// Return the register IDs owned by this frame's continuation.
    ///
    /// Frames store only fixed-width integer IDs; values and code ranges are
    /// resolved by the machine at the suspension boundary.
    pub(crate) fn register_ids(&self) -> Vec<u16> {
        match self {
            Self::Try { yield_dst, catch_slot, .. } => {
                let mut ids = vec![*yield_dst];
                if let Some(slot) = catch_slot {
                    ids.push(*slot);
                }
                ids
            }
            Self::Iterator { binding, yield_dst, slot, .. } => vec![*binding, *yield_dst, *slot],
            Self::Branch { dst, yield_dst, .. } => dst
                .iter()
                .copied()
                .chain(std::iter::once(*yield_dst))
                .collect(),
            Self::Private { yield_dst, .. } => vec![*yield_dst],
            Self::Loop { dst, yield_dst, per_iteration, .. } => {
                let mut ids = vec![*dst, *yield_dst];
                ids.extend(per_iteration.iter().copied());
                ids
            }
            Self::Await { .. } | Self::Delegate { .. } => Vec::new(),
            Self::Dispose { yield_dst, .. } => vec![*yield_dst],
        }
    }

    /// Check that every register ID addresses the machine's register window.
    pub(crate) fn has_valid_register_ids(&self, register_count: u16) -> bool {
        self.register_ids().into_iter().all(|id| id < register_count)
    }
}
fn set_resume<P>(
    phase: &mut P,
    resume: &mut CodeRange,
    destination: &mut u16,
    next_phase: P,
    next_resume: CodeRange,
    yield_destination: u16,
) {
    *phase = next_phase;
    *resume = next_resume;
    *destination = yield_destination;
}
