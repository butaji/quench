//! Node's junit reporter: suites, cases and comments form a tree built from the events in
//! order, and the tree is serialised once the stream ends. Child module of `report`.
use super::{Directive, TestReport};
use quench_runtime::units::MILLISECONDS_PER_SECOND;

/// One indent per nesting level, as Node's `treeToXML` writes them.
const INDENT: &str = "\t";
/// Node's junit reporter writes durations in seconds with six decimals.
const TIME_DECIMALS: usize = 6;

/// One node of the tree, referred to by its index in `Junit::nodes`.
struct Node {
    /// `None` for a suite that has not completed, which Node's root never does.
    tag: Option<&'static str>,
    attrs: Vec<(&'static str, String)>,
    nesting: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    comment: Option<String>,
    /// A failure's inspected error, written as its only content.
    text: Option<String>,
}

impl Node {
    fn new(tag: Option<&'static str>, nesting: usize, parent: Option<usize>) -> Self {
        Self { tag, attrs: Vec::new(), nesting, parent, children: Vec::new(), comment: None, text: None }
    }

    fn name(&self) -> &str {
        self.attrs.iter().find(|(key, _)| *key == "name").map_or("", |(_, value)| value.as_str())
    }

    /// Sets an attribute, keeping its position when it is already set.
    fn set(&mut self, key: &'static str, value: impl Into<String>) {
        let value = value.into();
        match self.attrs.iter_mut().find(|(name, _)| *name == key) {
            Some(entry) => entry.1 = value,
            None => self.attrs.push((key, value)),
        }
    }
}

/// The tree under construction: `current` is the open suite, `roots` the top level.
#[derive(Default)]
pub(super) struct Junit {
    nodes: Vec<Node>,
    roots: Vec<usize>,
    current: Option<usize>,
}

impl Junit {
    /// `startTest`: opens a suite under the open one, or at the top level.
    pub(super) fn open(&mut self, name: &str, nesting: usize) -> usize {
        let index = self.nodes.len();
        let mut node = Node::new(None, nesting, self.current);
        node.set("name", name);
        self.nodes.push(node);
        self.attach(index);
        self.current = Some(index);
        index
    }

    /// A `test:pass` or `test:fail`: closes the test's suite or case, opening it first when
    /// the open suite is not the test (the events came without `test:start`).
    pub(super) fn result(&mut self, report: &TestReport) {
        let open = match self.current {
            Some(open) => open,
            None => self.open("root", 0),
        };
        let test = if self.nodes[open].name() == report.name && self.nodes[open].nesting == report.nesting {
            open
        } else {
            self.open(&report.name, report.nesting)
        };
        self.current = self.nodes[test].parent;
        self.nodes[test].set("time", format!("{:.*}", TIME_DECIMALS, report.duration_ms / MILLISECONDS_PER_SECOND));
        let cases = self.cases_of(test);
        if cases.is_empty() {
            self.case(test, report);
        } else {
            self.suite(test, &cases);
        }
    }

    /// `test:diagnostic`: a comment in the open suite, or at the top level.
    pub(super) fn comment(&mut self, nesting: usize, message: &str) {
        let index = self.nodes.len();
        let mut node = Node::new(None, nesting, self.current);
        node.comment = Some(message.to_owned());
        self.nodes.push(node);
        self.attach(index);
    }

    /// The whole document after the last event: the top level, then the closing tag.
    pub(super) fn finish(&self) -> String {
        let mut out: String = self.roots.iter().map(|&index| self.render(index)).collect();
        out.push_str("</testsuites>\n");
        out
    }

    fn attach(&mut self, index: usize) {
        match self.current {
            Some(parent) => self.nodes[parent].children.push(index),
            None => self.roots.push(index),
        }
    }

    /// The children that are tests, not comments.
    fn cases_of(&self, test: usize) -> Vec<usize> {
        let children = self.nodes[test].children.iter();
        children.copied().filter(|&child| self.nodes[child].comment.is_none()).collect()
    }

    fn has_child(&self, index: usize, tag: &str) -> bool {
        self.nodes[index].children.iter().any(|&child| self.nodes[child].tag == Some(tag))
    }

    /// A suite of tests: its counts of tests, failures and skips.
    fn suite(&mut self, test: usize, cases: &[usize]) {
        let failures = cases.iter().filter(|&&case| self.has_child(case, "failure")).count();
        let skipped = cases.iter().filter(|&&case| self.has_child(case, "skipped")).count();
        let node = &mut self.nodes[test];
        node.tag = Some("testsuite");
        node.set("disabled", "0");
        node.set("errors", "0");
        node.set("tests", cases.len().to_string());
        node.set("failures", failures.to_string());
        node.set("skipped", skipped.to_string());
        node.set("hostname", hostname());
    }

    /// A test case: its skip or todo marker, then its failure.
    fn case(&mut self, test: usize, report: &TestReport) {
        self.nodes[test].tag = Some("testcase");
        self.nodes[test].set("classname", "test");
        match &report.directive {
            Some(Directive::Skip(message)) => self.leaf(test, report.nesting, "skip", message),
            Some(Directive::Todo(message)) => self.leaf(test, report.nesting, "todo", message),
            None => {}
        }
        if !report.passed {
            self.failure(test, report);
        }
    }

    /// A `skipped` child with the marker's type and message (`true` when there is none).
    fn leaf(&mut self, test: usize, nesting: usize, marker: &str, message: &str) {
        let index = self.add_child(test, nesting + 1, "skipped");
        let kind = if marker == "skip" { "skipped" } else { "todo" };
        let message = if message.is_empty() { "true" } else { message };
        self.nodes[index].set("type", kind);
        self.nodes[index].set("message", message);
    }

    /// A `failure` child holding the inspected error; Node's `undefined` when there is none.
    fn failure(&mut self, test: usize, report: &TestReport) {
        let (kind, message, text) = match &report.failure {
            Some(failure) => (failure.failure_type.as_str(), failure.message.as_str(), failure.inspected.as_str()),
            None => ("", "", "undefined"),
        };
        let kind = if kind.is_empty() { "undefined" } else { kind };
        let index = self.add_child(test, report.nesting + 1, "failure");
        self.nodes[index].set("type", kind);
        self.nodes[index].set("message", message);
        self.nodes[index].text = Some(text.to_owned());
        self.nodes[test].set("failure", message);
    }

    fn add_child(&mut self, parent: usize, nesting: usize, tag: &'static str) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node::new(Some(tag), nesting, Some(parent)));
        self.nodes[parent].children.push(index);
        index
    }

    /// Node's `treeToXML`: a comment, a self-closing element, or an element with its content.
    fn render(&self, index: usize) -> String {
        let node = &self.nodes[index];
        let indent = INDENT.repeat(node.nesting + 1);
        if let Some(comment) = &node.comment {
            return format!("{indent}<!-- {} -->\n", escape_comment(comment));
        }
        let attrs: Vec<String> =
            node.attrs.iter().map(|(key, value)| format!("{key}=\"{}\"", escape_attribute(value))).collect();
        let attrs = attrs.join(" ");
        let tag = node.tag.unwrap_or("undefined");
        if node.children.is_empty() && node.text.is_none() {
            return format!("{indent}<{tag} {attrs}/>\n");
        }
        let mut body: String = node.children.iter().map(|&child| self.render(child)).collect();
        if let Some(text) = &node.text {
            body.push_str(&format!("{}\n", escape_content(text)));
        }
        format!("{indent}<{tag} {attrs}>\n{body}{indent}</{tag}>\n")
    }
}

/// Node's `escapeContent`: `&` and `<`.
fn escape_content(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

/// Node's `escapeAttribute`: newlines removed, `"` replaced, then `escapeContent` (so the
/// `&` of `&quot;` is escaped again, as Node's output has it).
fn escape_attribute(text: &str) -> String {
    escape_content(&text.replace('\n', "").replace('"', "&quot;"))
}

fn escape_comment(text: &str) -> String {
    text.replace("--", "&#45;&#45;")
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().to_owned()).unwrap_or_default()
}
