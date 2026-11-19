//! A top-level function whose body was parsed lazily compiles as a stub, and compiles its body
//! as a chunk on its first call. The records here are the stub's data: the bindings it reads
//! from the scopes it closes over, and the program's stubs by id. The compiler builds them;
//! the bytecode carries and encodes them, so they live here rather than in the compiler.

use std::rc::Rc;
use super::append_vec::AppendVec;

/// The bindings a stub's function reads from the scopes it closes over, by name: each with the
/// level of its scope (the innermost is 0) and its slot, the names packed into one text so the
/// stub costs two allocations however many names it reads.
#[derive(Debug)]
pub(crate) struct ScopeNames {
    text: Box<str>,
    /// Each binding's end in `text`, its scope level and its slot.
    ends: Box<[(u32, u16, u16)]>,
}

impl ScopeNames {
    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, u16, u16)> {
        let starts = std::iter::once(0).chain(self.ends.iter().map(|&(end, ..)| end));
        starts.zip(self.ends.iter()).map(|(start, &(end, level, slot))| (&self.text[start as usize..end as usize], level, slot))
    }
}

impl<'n> FromIterator<(&'n str, u16, u16)> for ScopeNames {
    fn from_iter<I: IntoIterator<Item = (&'n str, u16, u16)>>(bindings: I) -> Self {
        let (mut text, mut ends) = (String::new(), Vec::new());
        for (name, level, slot) in bindings {
            text.push_str(name);
            ends.push((text.len() as u32, level, slot));
        }
        Self { text: text.into(), ends: ends.into() }
    }
}

/// What a stub's chunk closes over: how many scopes, the names it reads and whether it is strict.
#[derive(Debug)]
pub(crate) struct FirstCallScopes {
    pub(crate) levels: u16,
    pub(crate) names: ScopeNames,
    pub(crate) strict: bool,
}

impl FirstCallScopes {
    pub(crate) fn new(levels: u16, names: ScopeNames, strict: bool) -> Self {
        Self { levels, names, strict }
    }

    pub(crate) fn levels(&self) -> u16 {
        self.levels
    }

    pub(crate) fn names(&self) -> &ScopeNames {
        &self.names
    }

    pub(crate) fn strict(&self) -> bool {
        self.strict
    }
}

/// A program's nested stubs by id, sorted, each with the scopes its chunk compiles against; the
/// chunks of its stubs append theirs.
pub(crate) type FirstCallStubs = AppendVec<(u32, Rc<FirstCallScopes>)>;

/// A first-call chunk of a function whose body streams, compiled one run of its statements
/// at a time (`Vm::call_streamed_runs`): each run compiles as a chunk of its own that runs
/// once and is then dropped, so a body that runs once (mandreel's `global_init`, 614K
/// instructions) never has its code resident whole.
pub(crate) struct StreamRun {
    /// The start of the function in the program's text.
    pub(crate) start: u32,
    /// Where the run starts, for a run after the first, whose chunk holds the function's
    /// header alone; the first run's chunk compiles the function from its text.
    pub(crate) from: Option<(u32, StreamedRun)>,
    /// What the chunk's compilation found: the body, when it streams, and where its next run
    /// starts, if any.
    pub(crate) found: std::cell::Cell<Option<(StreamedRun, Option<u32>)>>,
}

/// The body of a function whose runs compile one at a time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamedRun {
    /// The offset of the body's opening brace, and of its closing one.
    pub(crate) open: u32,
    pub(crate) close: u32,
    pub(crate) strict: bool,
}
