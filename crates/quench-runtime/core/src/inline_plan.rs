#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialInlineDecision {
    Eligible,
    CapturesFrame,
    UsesArgumentsObject,
    ArityMismatch,
    NestedCall,
    ControlFlow,
    CodeSize,
    FrameSize,
}

pub const MAX_INITIAL_INLINE_CODE_BYTES: usize = 2_048;
pub const MAX_INITIAL_INLINE_FRAME_SLOTS: usize = 256;

#[derive(Clone, Copy, Debug)]
pub struct InitialInlineFacts {
    pub captures_frame: bool,
    pub uses_arguments_object: bool,
    pub exact_arity: bool,
    pub has_nested_call: bool,
    pub is_straight_line: bool,
    pub code_bytes: usize,
    pub frame_slots: usize,
}

pub fn classify_initial_inline_candidate(facts: InitialInlineFacts) -> InitialInlineDecision {
    use InitialInlineDecision::*;

    if facts.captures_frame {
        CapturesFrame
    } else if facts.uses_arguments_object {
        UsesArgumentsObject
    } else if !facts.exact_arity {
        ArityMismatch
    } else if facts.has_nested_call {
        NestedCall
    } else if !facts.is_straight_line {
        ControlFlow
    } else if facts.code_bytes > MAX_INITIAL_INLINE_CODE_BYTES {
        CodeSize
    } else if facts.frame_slots > MAX_INITIAL_INLINE_FRAME_SLOTS {
        FrameSize
    } else {
        Eligible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eligible_facts() -> InitialInlineFacts {
        InitialInlineFacts {
            captures_frame: false,
            uses_arguments_object: false,
            exact_arity: true,
            has_nested_call: false,
            is_straight_line: true,
            code_bytes: MAX_INITIAL_INLINE_CODE_BYTES,
            frame_slots: MAX_INITIAL_INLINE_FRAME_SLOTS,
        }
    }

    #[test]
    fn initial_inline_classifier_has_one_canonical_rejection() {
        let mut facts = eligible_facts();
        assert_eq!(
            classify_initial_inline_candidate(facts),
            InitialInlineDecision::Eligible
        );

        facts.captures_frame = true;
        facts.has_nested_call = true;
        assert_eq!(
            classify_initial_inline_candidate(facts),
            InitialInlineDecision::CapturesFrame
        );
    }

    #[test]
    fn initial_inline_budgets_accept_the_named_boundary() {
        let mut facts = eligible_facts();
        facts.code_bytes += 1;
        assert_eq!(
            classify_initial_inline_candidate(facts),
            InitialInlineDecision::CodeSize
        );

        facts = eligible_facts();
        facts.frame_slots += 1;
        assert_eq!(
            classify_initial_inline_candidate(facts),
            InitialInlineDecision::FrameSize
        );
    }
}
