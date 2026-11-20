//! Placing a linked body's rows in one code buffer: the entry thunk, the rows copied and patched
//! (`quench_bjit::copy_patch`), the exit stubs and routines behind them, and the symbolic fields
//! a loader fills.
use super::*;
use crate::vm::aot_image::Symbol;
use quench_bjit::EXIT_STUB_LEN;

/// A run-time address a linked body names by symbol: the code offset of its absolute word, which the
/// loader fills (`Symbol`), where the code holds zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) struct SymbolSite {
    pub(in crate::vm) offset: usize,
    pub(in crate::vm) symbol: Symbol,
}

/// The symbolic fields of `rows` (`symbols` by row, the symbol of each row's named hole), at their code
/// offsets from the code's start, `head` bytes past the entry thunk; a field must be an absolute word.
fn symbol_sites(rows: &[Placed], symbols: &[Symbolic], head: usize) -> Result<Vec<SymbolSite>, String> {
    let named = |row: usize, hole: u32| symbols.get(row).into_iter().flatten().flatten().find(|(index, _)| u32::from(*index) == hole).map(|(_, symbol)| *symbol);
    quench_bjit::symbolic_fields(rows, |row, hole| named(row, hole).is_some()).into_iter().map(|field| -> Result<SymbolSite, String> {
        if field.class != quench_bjit::ABS64 {
            return Err("a symbolic hole is not an absolute word".to_string());
        }
        let symbol = named(field.row, field.hole).ok_or("a symbolic field has no symbol")?;
        Ok(SymbolSite { offset: head + field.offset, symbol })
    }).collect()
}

/// Copies the rows behind the thunk, then the exit routines and each stub, and patches each
/// row's exits to their targets, `exits` by row and exit kind (a leave row's start, or a stub);
/// the code is position independent, so the buffer can move to executable memory. The rows are
/// in layout order: the hot rows first, so nothing cold sits between them. Also the code offset of
/// each row and, for a loop region, of the resume thunk (`entry_resume`), which follows the stubs;
/// each resume stub's `jumps` is patched to its row.
pub(in super::super) fn place_rows(ending: Ending, rows: &[Placed], (exits, routines): (&[Vec<Target>], &[Vec<u8>]), jumps: &[Jump], proven_row: usize, symbols: &[Symbolic]) -> Result<(Vec<u8>, Vec<usize>, usize, Vec<SymbolSite>), String> {
    let head = thunk(ending, 0, 0)?.len();
    let resume: &[u8] = if ending == Ending::Region { &routine("entry_resume")?.code } else { &[] };
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
    let proven_at = head + starts.get(proven_row).copied().unwrap_or(0);
    let thunk_bytes = thunk(ending, head, proven_at)?;
    code[..head].copy_from_slice(&thunk_bytes);
    code[at..at + resume.len()].copy_from_slice(resume);
    let resume_at = at;
    at += resume.len();
    code.truncate(at);
    Ok((code, starts.iter().map(|start| head + start).collect(), resume_at, sites))
}
