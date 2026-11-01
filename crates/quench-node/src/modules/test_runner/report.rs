//! The completed-test data Node's reporters consume, and the TAP and spec renderings of
//! it. A report is plain data: the in-process harness builds it from the test tree and a
//! `--test` coordinator reads it back from each file's child process.
use serde::{Deserialize, Serialize};

/// Node's `kSubtestsFailed`: a parent failing only through its children.
pub(crate) const SUBTESTS_FAILED: &str = "subtestsFailed";
const TAP_VERSION: u32 = 13;
const TAP_INDENT: &str = "    ";
const SPEC_INDENT: &str = "  ";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Test,
    Suite,
}

impl Kind {
    fn reported(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Suite => "suite",
        }
    }
}

/// `skip`/`todo` with the message the option or `t.skip()`/`t.todo()` gave (empty for none).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Directive {
    Skip(String),
    Todo(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Location {
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

/// A value as Node's TAP `jsToYaml` writes it, already formatted by `util.inspect`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Yaml {
    Null,
    Scalar(String),
    Lines(Vec<String>),
    Map(Vec<(String, Yaml)>),
    Circular,
}

/// A failing test's error: its TAP fields after `location`, and its spec inspection
/// (`util.inspect` of the unwrapped cause, plain and, for a colour terminal, coloured).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Failure {
    pub(crate) failure_type: String,
    pub(crate) fields: Vec<(String, Yaml)>,
    pub(crate) inspected: String,
    pub(crate) inspected_colored: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TestReport {
    pub(crate) name: String,
    pub(crate) nesting: usize,
    pub(crate) number: usize,
    pub(crate) kind: Kind,
    pub(crate) directive: Option<Directive>,
    pub(crate) passed: bool,
    pub(crate) cancelled: bool,
    pub(crate) duration_ms: f64,
    pub(crate) location: Option<Location>,
    pub(crate) failure: Option<Failure>,
    pub(crate) children: Vec<TestReport>,
    pub(crate) diagnostics: Vec<String>,
}

/// Node's harness counters, derived from reports.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counters {
    pub(crate) tests: usize,
    pub(crate) suites: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) cancelled: usize,
    pub(crate) skipped: usize,
    pub(crate) todo: usize,
    pub(crate) top_level: usize,
}

impl Counters {
    /// `countCompletedTest` over a report and its subtree.
    pub(crate) fn count(&mut self, report: &TestReport) {
        for child in &report.children {
            self.count(child);
        }
        if report.nesting == 0 {
            self.top_level += 1;
        }
        if report.kind == Kind::Suite {
            self.suites += 1;
            return;
        }
        match (&report.directive, report.cancelled, report.passed) {
            (Some(Directive::Skip(_)), _, _) => self.skipped += 1,
            (Some(Directive::Todo(_)), _, _) => self.todo += 1,
            (None, true, _) => self.cancelled += 1,
            (None, false, false) => self.failed += 1,
            (None, false, true) => self.passed += 1,
        }
        self.tests += 1;
    }

    pub(crate) fn success(&self) -> bool {
        self.failed == 0 && self.cancelled == 0
    }

    fn lines(&self, duration_ms: f64) -> [String; 8] {
        [
            format!("tests {}", self.tests),
            format!("suites {}", self.suites),
            format!("pass {}", self.passed),
            format!("fail {}", self.failed),
            format!("cancelled {}", self.cancelled),
            format!("skipped {}", self.skipped),
            format!("todo {}", self.todo),
            format!("duration_ms {}", number(duration_ms)),
        ]
    }
}

/// The reporter Node selects: `--test-reporter`, else spec on a terminal and TAP otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    Tap,
    Spec { colors: bool },
}

impl Style {
    pub(crate) fn select(requested: Option<&str>, terminal: bool) -> Result<Self, String> {
        match requested {
            None if terminal => Ok(Self::Spec { colors: true }),
            None | Some("tap") => Ok(Self::Tap),
            Some("spec") => Ok(Self::Spec { colors: terminal }),
            Some(other) => Err(other.to_owned()),
        }
    }
}

/// ANSI sequences of `internal/util/colors`, empty without colour.
#[derive(Clone, Copy)]
struct Palette {
    red: &'static str,
    green: &'static str,
    blue: &'static str,
    gray: &'static str,
    white: &'static str,
}

const COLORS: Palette = Palette {
    red: "\x1b[31m",
    green: "\x1b[32m",
    blue: "\x1b[34m",
    gray: "\x1b[90m",
    white: "\x1b[39m",
};
const PLAIN: Palette = Palette { red: "", green: "", blue: "", gray: "", white: "" };

const FAIL_SYMBOL: &str = "\u{2716} ";
const PASS_SYMBOL: &str = "\u{2714} ";
const INFO_SYMBOL: &str = "\u{2139} ";
const ARROW_SYMBOL: &str = "\u{25B6} ";
const SKIP_SYMBOL: &str = "\u{FE63} ";

/// One run's output: what each completed top-level test, passthrough line and the final
/// summary print, in Node's order.
pub(crate) struct Reporter {
    style: Style,
    counters: Counters,
    started: bool,
    failing: Vec<TestReport>,
    cwd: String,
}

impl Reporter {
    pub(crate) fn new(style: Style, cwd: String) -> Self {
        Self { style, counters: Counters::default(), started: false, failing: Vec::new(), cwd }
    }

    pub(crate) fn style(&self) -> Style {
        self.style
    }

    pub(crate) fn counters(&self) -> Counters {
        self.counters
    }

    fn header(&mut self, out: &mut String) {
        if !std::mem::replace(&mut self.started, true) && self.style == Style::Tap {
            out.push_str(&format!("TAP version {TAP_VERSION}\n"));
        }
    }

    pub(crate) fn test(&mut self, report: &TestReport) -> String {
        let mut out = String::new();
        self.header(&mut out);
        self.counters.count(report);
        match self.style {
            Style::Tap => tap_test(report, &mut out),
            Style::Spec { colors } => {
                spec_test(report, colors, &mut out);
                collect_failing(report, &mut self.failing);
            }
        }
        out
    }

    /// A child process's output line that is not a report (Node's `test:stdout`).
    pub(crate) fn output(&mut self, text: &str) -> String {
        let mut out = String::new();
        self.header(&mut out);
        match self.style {
            Style::Tap => {
                for line in text.split('\n').filter(|line| !line.is_empty()) {
                    out.push_str(&format!("# {}\n", tap_escape(line)));
                }
            }
            Style::Spec { .. } => out.push_str(text),
        }
        out
    }

    pub(crate) fn summary(&mut self, diagnostics: &[String], duration_ms: f64) -> String {
        let mut out = String::new();
        self.header(&mut out);
        let counts = self.counters.lines(duration_ms);
        let lines = diagnostics.iter().map(String::as_str).chain(counts.iter().map(String::as_str));
        match self.style {
            Style::Tap => {
                out.push_str(&format!("1..{}\n", self.counters.top_level));
                for line in lines {
                    out.push_str(&format!("# {}\n", tap_escape(line)));
                }
            }
            Style::Spec { colors } => {
                let palette = palette(colors);
                for line in lines {
                    out.push_str(&format!("{}{INFO_SYMBOL}{line}{}\n", palette.blue, palette.white));
                }
                out.push_str(&self.failing_section(colors));
            }
        }
        out
    }

    fn failing_section(&mut self, colors: bool) -> String {
        if self.failing.is_empty() {
            return String::new();
        }
        let palette = palette(colors);
        let mut parts = vec![format!("\n{}{FAIL_SYMBOL}failing tests:{}\n", palette.red, palette.white)];
        for test in std::mem::take(&mut self.failing) {
            if let Some(location) = &test.location {
                let relative = relative_path(&self.cwd, &location.file);
                parts.push(format!("test at {relative}:{}:{}", location.line, location.column));
            }
            parts.push(spec_line(&test, colors, "", false, true));
        }
        parts.join("\n")
    }
}

fn palette(colors: bool) -> Palette {
    if colors {
        COLORS
    } else {
        PLAIN
    }
}

/// A JavaScript Number's shortest round-trip text, as Node prints durations.
pub(crate) fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Node's `tapEscape`: its replacements in order, so a backslash an earlier one adds is
/// escaped again.
fn tap_escape(input: &str) -> String {
    const REPLACEMENTS: [(&str, &str); 8] = [
        ("\u{8}", "\\b"),
        ("\u{c}", "\\f"),
        ("\t", "\\t"),
        ("\n", "\\n"),
        ("\r", "\\r"),
        ("\u{b}", "\\v"),
        ("\\", "\\\\"),
        ("#", "\\#"),
    ];
    REPLACEMENTS.iter().fold(input.to_owned(), |text, (from, to)| text.replace(from, to))
}

fn tap_test(report: &TestReport, out: &mut String) {
    let indent = TAP_INDENT.repeat(report.nesting);
    out.push_str(&format!("{indent}# Subtest: {}\n", tap_escape(&report.name)));
    for child in &report.children {
        tap_test(child, out);
    }
    if !report.children.is_empty() {
        out.push_str(&format!("{indent}{TAP_INDENT}1..{}\n", report.children.len()));
    }
    let status = if report.passed { "ok" } else { "not ok" };
    let mut line = format!("{indent}{status} {} {}", report.number, tap_escape(&format!("- {}", report.name)));
    match &report.directive {
        Some(Directive::Skip(message)) => line.push_str(&directive_suffix("SKIP", message)),
        Some(Directive::Todo(message)) => line.push_str(&directive_suffix("TODO", message)),
        None => {}
    }
    out.push_str(&line);
    out.push('\n');
    out.push_str(&format!("{indent}  ---\n"));
    out.push_str(&format!("{indent}  duration_ms: {}\n", number(report.duration_ms)));
    out.push_str(&format!("{indent}  type: '{}'\n", report.kind.reported()));
    if let Some(failure) = &report.failure {
        if let Some(location) = &report.location {
            out.push_str(&format!(
                "{indent}  location: '{}:{}:{}'\n",
                location.file, location.line, location.column
            ));
        }
        for (key, value) in &failure.fields {
            yaml(&indent, key, value, out);
        }
    }
    out.push_str(&format!("{indent}  ...\n"));
    for diagnostic in &report.diagnostics {
        out.push_str(&format!("{indent}# {}\n", tap_escape(diagnostic)));
    }
}

fn directive_suffix(label: &str, message: &str) -> String {
    if message.is_empty() {
        format!(" # {label}")
    } else {
        format!(" # {label} {}", tap_escape(message))
    }
}

fn yaml(indent: &str, key: &str, value: &Yaml, out: &mut String) {
    match value {
        Yaml::Null => out.push_str(&format!("{indent}  {key}: ~\n")),
        Yaml::Scalar(text) => out.push_str(&format!("{indent}  {key}: {text}\n")),
        Yaml::Circular => out.push_str(&format!("{indent}  {key}: <Circular>\n")),
        Yaml::Lines(lines) => {
            out.push_str(&format!("{indent}  {key}: |-\n"));
            for line in lines {
                out.push_str(&format!("{indent}    {line}\n"));
            }
        }
        Yaml::Map(entries) => {
            out.push_str(&format!("{indent}  {key}:\n"));
            let nested = format!("{indent}  ");
            for (key, value) in entries {
                yaml(&nested, key, value, out);
            }
        }
    }
}

fn spec_test(report: &TestReport, colors: bool, out: &mut String) {
    let palette = palette(colors);
    let indent = SPEC_INDENT.repeat(report.nesting);
    if !report.children.is_empty() {
        out.push_str(&format!("{indent}{ARROW_SYMBOL}{}\n", report.name));
        for child in &report.children {
            spec_test(child, colors, out);
        }
    }
    out.push_str(&spec_line(report, colors, &indent, !report.children.is_empty(), false));
    out.push('\n');
    for diagnostic in &report.diagnostics {
        out.push_str(&format!("{}{indent}{INFO_SYMBOL}{diagnostic}{}\n", palette.blue, palette.white));
    }
}

/// Node's `formatTestReport`.
fn spec_line(report: &TestReport, colors: bool, indent: &str, has_children: bool, show_error: bool) -> String {
    let palette = palette(colors);
    let (mut color, mut symbol) = if report.passed {
        (palette.green, PASS_SYMBOL)
    } else {
        (palette.red, FAIL_SYMBOL)
    };
    let mut title = report.name.clone();
    if report.duration_ms != 0.0 {
        title.push_str(&format!(" {}({}ms){}", palette.gray, number(report.duration_ms), palette.white));
    }
    match &report.directive {
        Some(Directive::Skip(message)) => {
            title.push_str(&format!(" # {}", if message.is_empty() { "SKIP" } else { message }));
            color = palette.gray;
            symbol = SKIP_SYMBOL;
        }
        Some(Directive::Todo(message)) => {
            title.push_str(&format!(" # {}", if message.is_empty() { "TODO" } else { message }));
        }
        None => {}
    }
    let error = match (&report.failure, show_error) {
        (Some(failure), true) => {
            let inspected = match &failure.inspected_colored {
                Some(colored) if colors => colored,
                _ => &failure.inspected,
            };
            let message = inspected.replace("\r\n", "\n").replace('\n', &format!("\n{indent}  "));
            format!("\n{indent}  {message}\n")
        }
        _ => String::new(),
    };
    let error = match &report.failure {
        Some(failure) if has_children && !error.is_empty() && failure.failure_type != SUBTESTS_FAILED => {
            format!("\n{error}")
        }
        _ if has_children => String::new(),
        _ => error,
    };
    format!("{indent}{color}{symbol}{title}{}{error}", palette.white)
}

/// The failures Node's spec reporter lists at the end, children before their parent.
fn collect_failing(report: &TestReport, failing: &mut Vec<TestReport>) {
    for child in &report.children {
        collect_failing(child, failing);
    }
    if let Some(failure) = &report.failure {
        if failure.failure_type != SUBTESTS_FAILED {
            let mut leaf = report.clone();
            leaf.children.clear();
            failing.push(leaf);
        }
    }
}

/// `path.relative(from, to)` for absolute POSIX paths.
fn relative_path(from: &str, to: &str) -> String {
    let from = from.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    let target = to.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    if !to.starts_with('/') {
        return to.to_owned();
    }
    let common = from.iter().zip(&target).take_while(|(left, right)| left == right).count();
    let mut parts = vec![".."; from.len() - common];
    parts.extend(&target[common..]);
    parts.join("/")
}
