//! Flat rendering of the events Node's reporters print one at a time: `node:test/reporters`
//! and the dot and junit styles. A test prints when it completes, after its children; a
//! diagnostic or plan prints at its own position in the stream. Child module of `report`.
use super::{palette, spec_line, tap_escape, tap_result, number, Reporter, Style, TestReport};
use super::{INFO_SYMBOL, SPEC_INDENT, TAP_INDENT};

/// Node's dot reporter starts a new line after this many results (its non-terminal width).
const DOT_LINE_LENGTH: usize = 20;

impl Reporter {
    /// The text a stream starts with: TAP's version line or the XML prologue.
    pub(crate) fn start(&mut self) -> String {
        let mut out = String::new();
        self.header(&mut out);
        out
    }

    /// One completed test, printed without its children (they printed before it).
    pub(crate) fn event(&mut self, report: &TestReport) -> String {
        let mut out = String::new();
        match self.style {
            Style::Tap => tap_result(report, &mut out),
            Style::Spec { colors } => {
                out.push_str(&spec_line(report, colors, &SPEC_INDENT.repeat(report.nesting), false, false));
                out.push('\n');
            }
            Style::Dot => self.dot_mark(report.passed, &mut out),
            Style::Junit => self.junit.result(report),
        }
        if !report.passed && matches!(self.style, Style::Spec { .. } | Style::Dot) {
            let mut leaf = report.clone();
            leaf.children.clear();
            self.failing.push(leaf);
        }
        out
    }

    /// A report's subtree in event order: children first, each with its diagnostics.
    pub(super) fn flat_tree(&mut self, report: &TestReport, out: &mut String) {
        for child in &report.children {
            self.flat_tree(child, out);
        }
        out.push_str(&self.event(report));
        for diagnostic in &report.diagnostics {
            out.push_str(&self.diagnostic(report.nesting, diagnostic));
        }
    }

    /// `test:start`: TAP's subtest header.
    pub(crate) fn subtest(&mut self, nesting: usize, name: &str) -> String {
        match self.style {
            Style::Tap => format!("{}# Subtest: {}\n", TAP_INDENT.repeat(nesting), tap_escape(name)),
            Style::Junit => {
                self.junit.open(name, nesting);
                String::new()
            }
            _ => String::new(),
        }
    }

    /// `test:plan`: TAP's plan line.
    pub(crate) fn plan(&mut self, nesting: usize, count: f64) -> String {
        match self.style {
            Style::Tap => format!("{}1..{}\n", TAP_INDENT.repeat(nesting), number(count)),
            _ => String::new(),
        }
    }

    /// `test:diagnostic`: a message at its position in the stream.
    pub(crate) fn diagnostic(&mut self, nesting: usize, message: &str) -> String {
        match self.style {
            Style::Tap => format!("{}# {}\n", TAP_INDENT.repeat(nesting), tap_escape(message)),
            Style::Spec { colors } => {
                let palette = palette(colors);
                let indent = SPEC_INDENT.repeat(nesting);
                format!("{}{indent}{INFO_SYMBOL}{message}{}\n", palette.blue, palette.white)
            }
            Style::Junit => {
                self.junit.comment(nesting, message);
                String::new()
            }
            Style::Dot => String::new(),
        }
    }

    /// The text after the last event: spec's failing tests, dot's line end and failures,
    /// junit's closing tag. TAP prints nothing more.
    pub(crate) fn finish(&mut self) -> String {
        match self.style {
            Style::Tap => String::new(),
            Style::Spec { colors } => self.failing_section(colors),
            Style::Dot => self.dot_failures(),
            Style::Junit => self.junit.finish(),
        }
    }

    fn dot_mark(&mut self, passed: bool, out: &mut String) {
        out.push(if passed { '.' } else { 'X' });
        self.dot_column += 1;
        if self.dot_column == DOT_LINE_LENGTH {
            out.push('\n');
            self.dot_column = 0;
        }
    }

    fn dot_failures(&mut self) -> String {
        let mut out = String::from("\n");
        if self.failing.is_empty() {
            return out;
        }
        let palette = palette(false);
        out.push_str(&format!("\n{}Failed tests:{}\n\n", palette.red, palette.white));
        for test in std::mem::take(&mut self.failing) {
            out.push_str(&spec_line(&test, false, "", false, true));
        }
        out
    }
}

