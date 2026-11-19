//! Placing a linked body's rows in one code buffer: the entry thunk, the rows copied and patched
//! (`quench_bjit::copy_patch`), the exit stubs and routines behind them, and the symbolic fields
//! a loader fills.
use super::*;
use crate::vm::aot_image::Symbol;
use quench_bjit::{EXIT_STUB_LEN, exit_stub};

/// A run-time address a linked body names by symbol: the code offset of its absolute word, which the
/// loader fills (`Symbol`), where the code holds zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) struct SymbolSite {
    pub(in crate::vm) offset: usize,
    pub(in crate::vm) symbol: Symbol,
}

/// The symbolic fields of `rows` (`symbols` by row, the symbol of each row's named hole), at their code
/// offsets from the code's start, `head` bytes past the entry thunk; a field must be an absolute word.
fn symbol_sites(rows: &[Placed], symbols: &[Option<(u8, Symbol)>], head: usize) -> Result<Vec<SymbolSite>, String> {
    let named = |row: usize, hole: u32| symbols.get(row).copied().flatten().is_some_and(|(index, _)| u32::from(index) == hole);
    quench_bjit::symbolic_fields(rows, named).into_iter().map(|field| -> Result<SymbolSite, String> {
        if field.class != quench_bjit::ABS64 {
            return Err("a symbolic hole is not an absolute word".to_string());
        }
        let symbol = symbols[field.row].map(|(_, symbol)| symbol).ok_or("a symbolic field has no symbol")?;
        Ok(SymbolSite { offset: head + field.offset, symbol })
    }).collect()
}

/// Writes the exit stub of `pc` at `at`: it pushes the PC and jumps to the routine at `routine_at`.
fn write_stub(code: &mut [u8], at: usize, pc: u32, routine_at: usize) -> Result<(), String> {
    let stub = exit_stub(pc, routine_at as i64 - at as i64).map_err(|error| format!("exit stub: {error:?}"))?;
    code[at..at + EXIT_STUB_LEN].copy_from_slice(&stub);
    Ok(())
}

/// Copies each of `routines` to `at`, returning where each starts and the end.
fn append_routines(code: &mut [u8], mut at: usize, routines: &[Vec<u8>]) -> (Vec<usize>, usize) {
    let mut starts = Vec::with_capacity(routines.len());
    for routine in routines {
        code[at..at + routine.len()].copy_from_slice(routine);
        starts.push(at);
        at += routine.len();
    }
    (starts, at)
}

/// The address each exit of each row goes to, writing the stubs it names at `at` (each entering its
/// routine of `routine_at`); the end of the stubs too. `rows_at` is the address of the first row's
/// code and `starts` each row's offset from it.
fn exit_addresses(code: &mut [u8], (rows_at, starts): (usize, &[usize]), mut at: usize, (exits, routine_at): (&[Vec<Target>], &[usize])) -> Result<(Vec<Vec<usize>>, usize), String> {
    let base = code.as_ptr() as usize;
    let mut addresses: Vec<Vec<usize>> = Vec::with_capacity(exits.len());
    for row_exits in exits {
        let mut row_addresses = Vec::with_capacity(row_exits.len());
        for target in row_exits {
            row_addresses.push(match *target {
                Target::Row(row) => rows_at + starts[row],
                Target::Stub { pc, routine } => {
                    write_stub(code, at, pc, routine_at[routine])?;
                    at += EXIT_STUB_LEN;
                    base + at - EXIT_STUB_LEN
                }
            });
        }
        addresses.push(row_addresses);
    }
    Ok((addresses, at))
}

/// Copies the rows behind the thunk, then the exit routines and each stub, and patches each
/// row's exits to their targets, `exits` by row and exit kind (a leave row's start, or a stub);
/// the code is position independent, so the buffer can move to executable memory. The rows are
/// in layout order: the hot rows first, so nothing cold sits between them. Also the code offset of
/// each row and, for a loop region, of the resume thunk (`entry_resume`), which follows the stubs;
/// each resume stub's `jumps` is patched to its row.
pub(super) fn place_rows(ending: Ending, rows: &[Placed], (exits, routines): (&[Vec<Target>], &[Vec<u8>]), jumps: &[Jump], symbols: &[Option<(u8, Symbol)>]) -> Result<(Vec<u8>, Vec<usize>, usize, Vec<SymbolSite>), String> {
    let head = thunk(ending, 0)?.len();
    let resume = if ending == Ending::Region { routine("entry_resume")?.code } else { Vec::new() };
    let bound: usize = rows.iter().map(|row| row.stencil.code.len()).sum();
    let stubs = exits.iter().flatten().filter(|target| matches!(target, Target::Stub { .. })).count();
    let tail: usize = routines.iter().map(Vec::len).sum::<usize>() + stubs * EXIT_STUB_LEN + resume.len();
    let mut code = vec![0u8; head + bound + tail];
    let base = code.as_mut_ptr();
    // SAFETY: `base + head` has room for every stencil's full length, which bounds the rows.
    let (len, starts) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |_, _| base as usize) };
    let (routine_at, at) = append_routines(&mut code, head + len, routines);
    let (addresses, mut at) = exit_addresses(&mut code, (base as usize + head, &starts), at, (exits, &routine_at))?;
    // SAFETY: as above; the stubs lie after the copied code inside `code`.
    let (_, starts) = unsafe { quench_bjit::copy_patch(rows, base.add(head), |row, kind| addresses[row].get(kind as usize).copied().unwrap_or(base as usize)) };
    for jump in jumps {
        patch_jump(&mut code, head + starts[jump.row], starts[jump.target] as i64 - starts[jump.row] as i64)?;
    }
    let sites = symbol_sites(rows, symbols, head)?;
    let thunk_bytes = thunk(ending, head)?;
    code[..head].copy_from_slice(&thunk_bytes);
    code[at..at + resume.len()].copy_from_slice(&resume);
    let resume_at = at;
    at += resume.len();
    code.truncate(at);
    Ok((code, starts.iter().map(|start| head + start).collect(), resume_at, sites))
}
