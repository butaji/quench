//! Protocol misses of frame calls and memoized script calls (`protocol_family::FRAME_CALL`).

use super::super::*;

/// The miss body of a record of this family with static parameters `parameters`
/// (`protocol_row::<H, OP, ACC, { protocol_family::FRAME_CALL | parameters }>`). A memoized script
/// call (`Op::Call`) takes its memo shape (`MEMO_FIELD` .. `MEMO_FORWARD | MEMO_NESTED`) as its
/// parameters, which are the codes `script_call_memo_handler` installs.
pub(in crate::vm::dispatch_fast) const fn frame_call_miss_of<H: Host>(parameters: u8) -> Handler {
    match parameters {
        _ if parameters == MEMO_FIELD => script_call_memo_miss::<H, MEMO_FIELD> as Handler,
        _ if parameters == MEMO_ELEMENT => script_call_memo_miss::<H, MEMO_ELEMENT> as Handler,
        _ if parameters == MEMO_LENGTH => script_call_memo_miss::<H, MEMO_LENGTH> as Handler,
        _ if parameters == MEMO_NESTED => script_call_memo_miss::<H, MEMO_NESTED> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_FIELD => script_call_memo_miss::<H, { MEMO_FORWARD | MEMO_FIELD }> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_ELEMENT => script_call_memo_miss::<H, { MEMO_FORWARD | MEMO_ELEMENT }> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_LENGTH => script_call_memo_miss::<H, { MEMO_FORWARD | MEMO_LENGTH }> as Handler,
        _ if parameters == MEMO_FORWARD | MEMO_NESTED => script_call_memo_miss::<H, { MEMO_FORWARD | MEMO_NESTED }> as Handler,
        _ => lane_general_step::<H>,
    }
}
