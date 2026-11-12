//! The residual counts of a stitched b-jit loop (`super::super::residuals`): its rows, the
//! frame traffic of its registers and the exits its rows can take.

use super::super::residuals::{report, Residual};
use super::*;

/// Whether a frame slot holds the register's home.
fn in_frame(place: Place) -> bool {
    matches!(place, Place::Mem | Place::MemInt)
}

/// The tag stores and the other frame traffic of `row`: its result into a tagged slot is a box,
/// any other read or write of a frame home a spill.
fn frame_traffic(layout: &Layout, row: &Decoded) -> (u32, u32) {
    let place = |register: Register| layout.homes.place(register);
    let result = row.dst.map(place);
    let reads = [
        row.a.and_then(register_of),
        row.b.and_then(register_of),
        row.base,
    ]
    .into_iter()
    .flatten();
    let boxes = u32::from(result == Some(Place::Mem));
    let spills = u32::from(result == Some(Place::MemInt))
        + reads.filter(|read| in_frame(place(*read))).count() as u32;
    (boxes, spills)
}

/// Whether `row` can leave the loop through a check: a row that proves something, or a branch
/// out of the region. The back edge and the branches inside are no checks.
fn is_check(layout: &Layout, row: &Decoded, planned: &Planned) -> bool {
    planned.exit_pc.is_some()
        && row.target.is_none_or(|target| {
            row.kind != Kind::Jump && !(layout.header..=layout.backedge).contains(&target)
        })
}

/// The counts of the loop `body` plans for `decoded`. Rows a tile covers have no code and
/// count nothing; a stencil row never calls (a failed proof leaves through its exit). The
/// loop's own test is one of the checks and is not a guard.
fn of_rows(layout: &Layout, decoded: &[Decoded], body: &[Planned]) -> Residual {
    let mut residual = Residual::default();
    let mut checks = 0;
    for (row, planned) in decoded
        .iter()
        .zip(body)
        .filter(|(_, planned)| !planned.stencil.code.is_empty())
    {
        let moves = (planned.pre.len() + planned.post.len()) as u32;
        let (boxes, spills) = frame_traffic(layout, row);
        residual.instructions += 1 + moves;
        residual.boxes += boxes;
        residual.spills += spills + moves;
        checks += u32::from(is_check(layout, row, planned));
    }
    residual.guards = checks.saturating_sub(1);
    residual
}

/// Reports the region's counts and its code `bytes` when the channel is on.
pub(super) fn report_rows(layout: &Layout, decoded: &[Decoded], body: &[Planned], bytes: usize) {
    if crate::trace::Channel::Residuals.on() {
        report(
            "b-jit",
            layout.header,
            of_rows(layout, decoded, body),
            Some(bytes),
        );
    }
}
