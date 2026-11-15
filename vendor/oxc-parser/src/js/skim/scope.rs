//! The scopes of a skimmed body: what each declares, and the references still to settle.
use super::Seen;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// What a name was declared as in a scope.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Decl {
    Var,
    Function,
    Param,
    /// The name of a function expression, bound around its own scope: any declaration of
    /// the same name in the function replaces it.
    Name,
    Lexical,
    /// A function declared in a block.
    BlockFunction,
    Catch,
}

impl Decl {
    /// A `var` of the same name in a scope inside this one is an error.
    fn blocks_var(self) -> bool {
        matches!(self, Self::Lexical | Self::BlockFunction | Self::Catch)
    }
}

struct Frame {
    serial: u32,
    refs_start: usize,
    function: bool,
    declared: bool,
}

/// The state a failed attempt returns the scopes to.
pub(super) struct Mark {
    frames: usize,
    refs: usize,
}

#[derive(Default)]
pub(super) struct Scopes<'a> {
    frames: Vec<Frame>,
    /// Declarations by scope serial and name: serials are never reused, so a closed or
    /// abandoned scope's entries are never read again.
    decls: FxHashMap<(u32, &'a str), Decl>,
    /// References whose scope is still open, in reading order.
    pub(super) refs: Vec<&'a str>,
    serial: u32,
}

impl<'a> Scopes<'a> {
    pub(super) fn enter(&mut self, function: bool) {
        self.serial += 1;
        self.frames.push(Frame { serial: self.serial, refs_start: self.refs.len(), function, declared: false });
    }

    /// Closes the innermost scope: the references it declares are settled, the rest stay for
    /// the scopes around it; a function's are sorted and without repeats.
    pub(super) fn exit(&mut self) {
        let Some(frame) = self.frames.pop() else { return };
        let (start, mut end) = (frame.refs_start, self.refs.len());
        if frame.declared {
            end = self.retain_undeclared(start, frame.serial);
        }
        if frame.function {
            end = self.sort_unique(start, end);
        }
        self.refs.truncate(end);
    }

    /// Compacts `refs[start..]` to the names not declared in scope `serial`: the new end.
    fn retain_undeclared(&mut self, start: usize, serial: u32) -> usize {
        let mut kept = start;
        for read in start..self.refs.len() {
            let name = self.refs[read];
            if !self.decls.contains_key(&(serial, name)) {
                self.refs[kept] = name;
                kept += 1;
            }
        }
        kept
    }

    /// Sorts `refs[start..end]` and compacts its repeats: the new end.
    fn sort_unique(&mut self, start: usize, end: usize) -> usize {
        self.refs[start..end].sort_unstable();
        let mut kept = start;
        for read in start..end {
            if kept == start || self.refs[read] != self.refs[kept - 1] {
                self.refs[kept] = self.refs[read];
                kept += 1;
            }
        }
        kept
    }

    pub(super) fn reference(&mut self, name: &'a str) {
        self.refs.push(name);
    }

    pub(super) fn mark(&self) -> Mark {
        Mark { frames: self.frames.len(), refs: self.refs.len() }
    }

    pub(super) fn reset(&mut self, mark: Mark) {
        self.frames.truncate(mark.frames);
        self.refs.truncate(mark.refs);
    }

    /// The innermost scope is a function's, not a block's.
    pub(super) fn at_function(&self) -> bool {
        self.frames.last().is_some_and(|frame| frame.function)
    }

    /// `name` declared in the innermost scope, unless a declaration there conflicts.
    fn put(&mut self, name: &'a str, decl: Decl, conflicts: fn(Decl) -> bool) -> Seen {
        let frame = self.frames.last_mut()?;
        frame.declared = true;
        match self.decls.entry((frame.serial, name)) {
            Entry::Occupied(mut held) if !conflicts(*held.get()) => {
                held.insert(decl);
            }
            Entry::Occupied(_) => return None,
            Entry::Vacant(slot) => {
                slot.insert(decl);
            }
        }
        Some(())
    }

    /// A `var` in a block or a function: it conflicts with a lexical binding of the name in
    /// any scope up to its function's.
    pub(super) fn declare_var(&mut self, name: &'a str) -> Seen {
        for frame in self.frames.iter_mut().rev() {
            match self.decls.entry((frame.serial, name)) {
                Entry::Occupied(held) if held.get().blocks_var() => return None,
                Entry::Occupied(mut held) if *held.get() == Decl::Name => {
                    held.insert(Decl::Var);
                }
                Entry::Occupied(_) => {}
                Entry::Vacant(slot) => {
                    slot.insert(Decl::Var);
                }
            }
            frame.declared = true;
            if frame.function {
                break;
            }
        }
        Some(())
    }

    pub(super) fn declare_lexical(&mut self, name: &'a str) -> Seen {
        self.put(name, Decl::Lexical, |held| held != Decl::Name)
    }

    pub(super) fn declare_param(&mut self, name: &'a str) -> Seen {
        self.put(name, Decl::Param, |held| held != Decl::Name)
    }

    pub(super) fn declare_catch(&mut self, name: &'a str) -> Seen {
        self.put(name, Decl::Catch, |held| held != Decl::Name)
    }

    /// A function declared where the innermost scope lies: a function's top level acts as a
    /// `var`, a block's as a lexical binding that no other declaration repeats.
    pub(super) fn declare_function(&mut self, name: &'a str) -> Seen {
        if self.at_function() {
            self.put(name, Decl::Function, Decl::blocks_var)
        } else {
            self.put(name, Decl::BlockFunction, |held| held != Decl::Name)
        }
    }

    /// The name a function expression has inside itself.
    pub(super) fn declare_name(&mut self, name: &'a str) {
        let _ = self.put(name, Decl::Name, |_| false);
    }
}
