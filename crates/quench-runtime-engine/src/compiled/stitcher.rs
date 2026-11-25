//! The one stitcher every b-jit family places its rows through: the JavaScript bodies and regions
//! (`base_region::link`), the Wasm functions (`wasm_stitch`) and the regexp programs
//! (`regexp_native::stitch`). A family plans its rows (library stencils, their holes, their taken
//! rows) and its routines; this places them: a routine copied with its holes and its calls of the
//! first row patched, the rows copied and patched (`quench_bjit::copy_patch`) with each exit
//! resolved to an address or to another row of the same code. One placement means a family gains
//! every improvement to it (layout, cold parts, reach) at once.

use quench_bjit::{HOLES, Row, Stencil};

/// Where a row's exit goes: an address, or a row of the same rows (its start once placed).
#[derive(Clone, Copy)]
pub(crate) enum Exit {
    Address(usize),
    Row(usize),
}

/// A library stencil by name (a row or a routine), where this target's library has it.
pub(crate) fn library(name: &str) -> Option<Stencil> {
    quench_bjit::rustc::stencil(name)
}

/// A routine by name, kept for the process's life: the routines are few and every stitch asks.
pub(crate) fn routine(name: &'static str) -> Result<&'static Stencil, String> {
    static ROUTINES: quench_bjit::rustc::Memo<&'static str> = std::sync::OnceLock::new();
    quench_bjit::rustc::memoized(&ROUTINES, name, || library(name).ok_or_else(|| format!("no stencil routine {name}")))
}

/// Copies `routine` into `code` at `start`: its value holes from `holes`, each of its successors
/// (its call or jump into the rows) the code offset `successor`.
pub(crate) fn place_routine(code: &mut [u8], start: usize, routine: &Stencil, holes: &[u64; HOLES], successor: usize) -> Result<(), String> {
    code[start..start + routine.code.len()].copy_from_slice(&routine.code);
    for reloc in &routine.relocs {
        let at = start + reloc.offset as usize;
        if quench_bjit::class::is_successor(reloc.class) {
            quench_bjit::patch_successor(code, at, reloc.class, successor as i64 + reloc.addend - at as i64).map_err(|error| format!("routine successor: {error:?}"))?;
        } else {
            quench_bjit::patch_value(code, at, reloc.class, holes[reloc.index as usize]);
        }
    }
    Ok(())
}

/// The bytes `rows` take placed (`place_rows`), and each row's start from the first.
pub(crate) fn placement(rows: &[Row]) -> (usize, Vec<usize>) {
    quench_bjit::placement(rows)
}

/// The room `rows` need before placement drops their joins: every stencil's full length.
pub(crate) fn room(rows: &[Row]) -> usize {
    rows.iter().map(|row| row.stencil.code.len()).sum()
}

/// Copies `rows` into `code` at `at` and patches them, `exit` giving each row's exit of a kind.
/// Returns the placed length and each row's code offset (from the code's start). The code is
/// position independent: it may move to executable memory afterwards.
pub(crate) fn place_rows(code: &mut [u8], at: usize, rows: &[Row], exit: impl Fn(usize, u32) -> Exit) -> (usize, Vec<usize>) {
    assert!(code.len() >= at + room(rows), "the code has room for every row");
    let (len, starts) = placement(rows);
    let base = code.as_ptr() as usize + at;
    let address = |target: Exit| match target {
        Exit::Address(address) => address,
        Exit::Row(row) => base + starts[row],
    };
    // SAFETY: `code` holds every stencil's full length past `at` (asserted above), which bounds the copy.
    let (copied, copied_starts) = unsafe { quench_bjit::copy_patch(rows, code.as_mut_ptr().add(at), |row, kind| address(exit(row, kind))) };
    debug_assert_eq!((copied, &copied_starts), (len, &starts));
    (len, starts.into_iter().map(|start| at + start).collect())
}

/// Where every row's exit goes in a unit `stitch_behind` lays out: one of its routines, or one of
/// its rows.
#[derive(Clone, Copy)]
pub(crate) enum Behind {
    Routine(usize),
    Row(usize),
}

/// Routines then rows in one code buffer, as the Wasm and regexp families lay a unit out: the entry
/// routine first (its holes, its call the first row), then `routines` copied as they are, then the
/// rows, every exit going to `exit`. Returns the code, each routine's offset and each row's.
pub(crate) fn stitch_behind(entry: (&Stencil, &[u64; HOLES]), routines: &[&Stencil], rows: &[Row], exit: Behind) -> Result<(Vec<u8>, Vec<usize>, Vec<usize>), String> {
    let mut routine_at = Vec::with_capacity(routines.len());
    let mut rows_at = entry.0.code.len();
    for routine in routines {
        routine_at.push(rows_at);
        rows_at += routine.code.len();
    }
    let mut code = vec![0u8; rows_at + room(rows)];
    place_routine(&mut code, 0, entry.0, entry.1, rows_at)?;
    for (routine, at) in routines.iter().zip(&routine_at) {
        code[*at..*at + routine.code.len()].copy_from_slice(&routine.code);
    }
    let exit = match exit {
        Behind::Routine(index) => Exit::Address(code.as_ptr() as usize + routine_at[index]),
        Behind::Row(row) => Exit::Row(row),
    };
    let (len, starts) = place_rows(&mut code, rows_at, rows, |_, _| exit);
    code.truncate(rows_at + len);
    Ok((code, routine_at, starts))
}
