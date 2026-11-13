//! Labels and the statements `break` and `continue` may leave, within one function.
use super::{Seen, Skim};
use crate::{ParserConfig as Config, lexer::Kind};

#[derive(Default)]
pub(super) struct Flow<'a> {
    /// The labels around the current statement, with whether each labels a loop.
    labels: Vec<(&'a str, bool)>,
    /// Labels directly before the statement about to parse.
    run: usize,
    pub(super) loops: u32,
    pub(super) switches: u32,
}

impl<'a, C: Config> Skim<'_, 'a, C> {
    /// Takes the labels attached to the statement about to parse.
    pub(super) fn take_label_run(&mut self) -> usize {
        std::mem::take(&mut self.flow.run)
    }

    /// The statement the last `run` labels are attached to is a loop.
    pub(super) fn mark_loop_labels(&mut self, run: usize) {
        let start = self.flow.labels.len() - run;
        self.flow.labels[start..].iter_mut().for_each(|label| label.1 = true);
    }

    /// Reads the label `name:` and parses the statement it labels.
    pub(super) fn labelled(&mut self, name: &'a str, run: usize) -> Seen {
        if self.flow.labels.iter().any(|(held, _)| *held == name) {
            return None;
        }
        self.flow.labels.push((name, false));
        self.flow.run = run + 1;
        let seen = self.statement();
        self.flow.labels.pop();
        seen
    }

    /// `break` or `continue`, with the label it names if any.
    pub(super) fn jump(&mut self, kind: Kind) -> Seen {
        self.bump();
        if !self.newline() && super::is_plain_identifier(self.kind()) {
            let name = self.identifier()?;
            let label = self.flow.labels.iter().find(|(held, _)| *held == name)?;
            if kind == Kind::Continue && !label.1 {
                return None;
            }
        } else if self.flow.loops == 0 && (kind == Kind::Continue || self.flow.switches == 0) {
            return None;
        }
        self.semicolon()
    }
}
