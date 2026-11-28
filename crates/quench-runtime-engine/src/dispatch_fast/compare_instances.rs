//! The comparison jump rows a record takes once `specialize_compare_jumps` gives it its instance: the flagged
//! instances from `FLAG_COMPARE` on (operator, formed, nullish, Boolean and proven) and the guarded rows. Each
//! instance decodes the record's own comparison, so a back edge's fused target run tests a formed loop header
//! as it tests the plain row (`part_5::target_run_holds`, `update_fusion`).

use super::{lane_frame::{compare_guarded_instances, FLAG_COMPARE}, Handler};

/// The flagged comparison jump instances of direction `BACKWARD`: one row per flag from `FLAG_COMPARE` through the
/// last instance (the proven ones end the flag space, at `FLAG_COMPARE + 51`).
fn instance_rows<const BACKWARD: bool>() -> [Handler; 52] {
    macro_rules! row { ($offset:literal) => { row_jump!({ quench_runtime::staged::COMPARE_JUMP_ROW }, BACKWARD, { FLAG_COMPARE + $offset }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }) as Handler } }
    macro_rules! rows { ($($n:literal)+) => { [$(row!($n)),+] } }
    rows!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51)
}

fn holds(rows: &[Handler], handler: *const ()) -> bool {
    rows.iter().any(|row| *row as *const () == handler)
}

/// Whether `handler` is a comparison jump's row at an instance `specialize_compare_jumps` may give its record. The
/// plain row is not one: `plain_row` tests that.
pub(crate) fn is_compare_instance(handler: *const ()) -> bool {
    holds(&instance_rows::<true>(), handler) || holds(&instance_rows::<false>(), handler)
        || holds(&compare_guarded_instances::<true>(), handler) || holds(&compare_guarded_instances::<false>(), handler)
}
