//! The prepared subjects of recently matched strings (`Vm::regexp_subject`), kept between
//! matches until the next collection.
use super::*;

/// Runs of shared storage (a slice, an append buffer's prefix) whose copies are kept: a
/// scanner's input and the piece it is working on.
const RUN_SUBJECTS: usize = 2;

#[derive(Default)]
pub(in crate::vm) struct SubjectCache {
    /// Whole strings whose search decodes them, most recent first.
    pub(super) whole: Vec<Rc<quench_regexp::Subject>>,
    /// Runs matched, most recent first, each with the whole-storage copy a matcher subject
    /// owns. The run is held, so its storage's address names it for as long as it is kept.
    runs: Vec<(FlatUnits, Rc<quench_regexp::Subject>)>,
}

impl SubjectCache {
    pub(in crate::vm) fn clear(&mut self) {
        self.whole.clear();
        self.runs.clear();
    }

    /// The subject of `run`, a wide slice or an append buffer's prefix: its copy into whole
    /// storage, made once per run and found again by the run's storage. A scanner searches its
    /// input from each token's end, and a copy per search is the input's length per token
    /// (postcss-wtb, 2026-11-10: 475K searches copied 190 KB each, 18.5% of the run).
    pub(super) fn run(&mut self, run: &FlatUnits) -> PreparedSubject {
        if let Some(position) = self.runs.iter().position(|(kept, _)| kept.same_storage(run)) {
            self.runs[..=position].rotate_right(1);
            return PreparedSubject::Shared(Rc::clone(&self.runs[0].1));
        }
        let subject = Rc::new(match run.view() {
            crate::vm::wtf16::Units::Wide(units) => quench_regexp::Subject::new(Rc::from(units)),
            crate::vm::wtf16::Units::Ascii(bytes) => {
                // SAFETY: ASCII bytes are UTF-8.
                let text: Rc<str> = Rc::from(unsafe { std::str::from_utf8_unchecked(bytes) });
                let length = text.len();
                quench_regexp::Subject::from_ascii(text, 0..length)
            }
        });
        self.runs.truncate(RUN_SUBJECTS - 1);
        self.runs.insert(0, (run.clone(), Rc::clone(&subject)));
        PreparedSubject::Shared(subject)
    }
}
