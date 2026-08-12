impl Frame {
    fn advance_resume(&mut self, range: CodeRange, yield_dst: u16) -> bool {
        match self {
            Self::Try { phase, body_resume, yield_dst: dst, .. } => {
                set_resume(phase, body_resume, dst, TryPhase::Body, range, yield_dst)
            }
            Self::Iterator { phase, body_resume, yield_dst: dst, .. } => {
                set_resume(phase, body_resume, dst, IteratorPhase::Body, range, yield_dst)
            }
            Self::Branch { phase, branch_resume, yield_dst: dst, .. } => {
                set_resume(phase, branch_resume, dst, BranchPhase::Body, range, yield_dst)
            }
            Self::Private { phase, body_resume, yield_dst: dst, .. } => {
                set_resume(phase, body_resume, dst, PrivatePhase::Body, range, yield_dst)
            }
            _ => return false,
        }
        true
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
