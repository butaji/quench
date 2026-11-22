//! Protocol misses of frame calls and memoized script calls (`protocol_family::FRAME_CALL`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row!(OP, ACC, { protocol_family::FRAME_CALL | parameters })`). A memoized script
/// call (`Op::Call`) takes its memo shape (`MEMO_FIELD` .. `MEMO_FORWARD | MEMO_NESTED`) as its
/// parameters, which are the codes `script_call_memo_handler` installs.
pub(crate) const fn frame_call_miss_of(parameters: u8) -> Handler {
    match parameters {
        _ if parameters == MEMO_FIELD => const { script_call_memo_miss_handler::<MEMO_FIELD>() },
        _ if parameters == MEMO_ELEMENT => const { script_call_memo_miss_handler::<MEMO_ELEMENT>() },
        _ if parameters == MEMO_LENGTH => const { script_call_memo_miss_handler::<MEMO_LENGTH>() },
        _ if parameters == MEMO_NESTED => const { script_call_memo_miss_handler::<MEMO_NESTED>() },
        _ if parameters == MEMO_FORWARD | MEMO_FIELD => const { script_call_memo_miss_handler::<{ MEMO_FORWARD | MEMO_FIELD }>() },
        _ if parameters == MEMO_FORWARD | MEMO_ELEMENT => const { script_call_memo_miss_handler::<{ MEMO_FORWARD | MEMO_ELEMENT }>() },
        _ if parameters == MEMO_FORWARD | MEMO_LENGTH => const { script_call_memo_miss_handler::<{ MEMO_FORWARD | MEMO_LENGTH }>() },
        _ if parameters == MEMO_FORWARD | MEMO_NESTED => const { script_call_memo_miss_handler::<{ MEMO_FORWARD | MEMO_NESTED }>() },
        _ => general_step_in_lane,
    }
}
