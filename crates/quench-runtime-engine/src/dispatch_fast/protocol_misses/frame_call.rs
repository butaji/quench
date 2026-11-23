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
        _ if parameters == MEMO_FIELD => script_call_memo_miss::<MEMO_FIELD> as Handler,
        _ if parameters == MEMO_ELEMENT => script_call_memo_miss::<MEMO_ELEMENT> as Handler,
        _ if parameters == MEMO_LENGTH => script_call_memo_miss::<MEMO_LENGTH> as Handler,
        _ if parameters == MEMO_NESTED => script_call_memo_miss::<MEMO_NESTED> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_FIELD => script_call_memo_miss::<{ MEMO_FORWARD | MEMO_FIELD }> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_ELEMENT => script_call_memo_miss::<{ MEMO_FORWARD | MEMO_ELEMENT }> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_LENGTH => script_call_memo_miss::<{ MEMO_FORWARD | MEMO_LENGTH }> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_NESTED => script_call_memo_miss::<{ MEMO_FORWARD | MEMO_NESTED }> as Handler,
        _ => general_step_in_lane,
    }
}
