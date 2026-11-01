//! `node:test`: the test tree, its contexts and its reporters, owned by Rust.
//!
//! Registration builds a tree of [`Node`]s; [`run`] drives each through Node's test
//! lifecycle (hooks, body, subtests) as promise reactions settle; a finished top-level
//! test becomes a [`report::TestReport`] the selected reporter renders.
pub(crate) mod cli;
mod failure;
pub(crate) mod mock;
pub(crate) mod report;
pub(crate) mod run;
pub(crate) mod wait_for;

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::time::Instant;

type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) type NodeId = usize;

/// The tree's root (`<root>`), created with the harness.
const ROOT: NodeId = 0;
/// The internal slot naming the node a TestContext or SuiteContext belongs to.
const NODE_SLOT: &str = "\0quench:test:node";
/// Node's `TIMEOUT_MAX`, the largest timer delay.
const TIMEOUT_MAX_MS: f64 = 2_147_483_647.0;
/// Node's message for `only` without `--test-only`.
const ONLY_WARNING: &str = "'only' and 'runOnly' require the --test-only command-line option.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Root,
    Suite,
    Test,
}

/// What a registration function creates and the option it forces (`test.skip`, `it.only`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Override {
    None,
    Skip,
    Todo,
    Only,
}

/// The registration functions `node:test` exports: (data tag, created kind, override).
const FACTORIES: [(&str, Kind, Override); 8] = [
    ("test", Kind::Test, Override::None),
    ("test.skip", Kind::Test, Override::Skip),
    ("test.todo", Kind::Test, Override::Todo),
    ("test.only", Kind::Test, Override::Only),
    ("suite", Kind::Suite, Override::None),
    ("suite.skip", Kind::Suite, Override::Skip),
    ("suite.todo", Kind::Suite, Override::Todo),
    ("suite.only", Kind::Suite, Override::Only),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HookKind {
    Before,
    After,
    BeforeEach,
    AfterEach,
}

const HOOKS: [(&str, HookKind); 4] = [
    ("before", HookKind::Before),
    ("after", HookKind::After),
    ("beforeEach", HookKind::BeforeEach),
    ("afterEach", HookKind::AfterEach),
];

#[derive(Default)]
pub(crate) struct Hooks {
    before: Vec<RootId>,
    after: Vec<RootId>,
    before_each: Vec<RootId>,
    after_each: Vec<RootId>,
}

impl Hooks {
    fn list(&self, kind: HookKind) -> &[RootId] {
        match kind {
            HookKind::Before => &self.before,
            HookKind::After => &self.after,
            HookKind::BeforeEach => &self.before_each,
            HookKind::AfterEach => &self.after_each,
        }
    }

    fn list_mut(&mut self, kind: HookKind) -> &mut Vec<RootId> {
        match kind {
            HookKind::Before => &mut self.before,
            HookKind::After => &mut self.after,
            HookKind::BeforeEach => &mut self.before_each,
            HookKind::AfterEach => &mut self.after_each,
        }
    }
}

pub(crate) struct Node {
    parent: Option<NodeId>,
    name: String,
    kind: Kind,
    nesting: usize,
    number: usize,
    body: Option<RootId>,
    directive: Option<report::Directive>,
    location: Option<report::Location>,
    hooks: Hooks,
    children: Vec<NodeId>,
    /// Children started so far; they run one at a time, in registration order.
    started_children: usize,
    /// Children given a test number: those started and not filtered out.
    numbered_children: usize,
    only: bool,
    /// Node's `runOnlySubtests`: only children marked `only` run (under `--test-only`).
    run_only: bool,
    /// Node's `hasOnlyTests`: a descendant is marked `only`.
    has_only: bool,
    /// Filtered out by `--test-only`: never run or reported.
    filtered: bool,
    running_child: Option<NodeId>,
    step: run::Step,
    /// Identifies the settlement this node awaits; a stale reaction carries an older one.
    awaiting: u32,
    ran_before: bool,
    started: Option<Instant>,
    duration_ms: f64,
    failure: Option<report::Failure>,
    cancelled: bool,
    finished: bool,
    context: Option<RootId>,
    completion: Option<RootId>,
    build: Option<RootId>,
    before_pending: Option<RootId>,
    timeout: Option<f64>,
    timer: Option<RootId>,
    diagnostics: Vec<String>,
    plan: Option<usize>,
    assertions: usize,
    assert: Option<RootId>,
    mock: Option<RootId>,
    signal: Option<RootId>,
}

impl Node {
    fn new(parent: Option<NodeId>, name: String, kind: Kind, nesting: usize) -> Self {
        Self {
            parent,
            name,
            kind,
            nesting,
            number: 0,
            body: None,
            directive: None,
            location: None,
            hooks: Hooks::default(),
            children: Vec::new(),
            started_children: 0,
            numbered_children: 0,
            only: false,
            run_only: false,
            has_only: false,
            filtered: false,
            running_child: None,
            step: run::Step::Pending,
            awaiting: 0,
            ran_before: false,
            started: None,
            duration_ms: 0.0,
            failure: None,
            cancelled: false,
            finished: false,
            context: None,
            completion: None,
            build: None,
            before_pending: None,
            timeout: None,
            timer: None,
            diagnostics: Vec::new(),
            plan: None,
            assertions: 0,
            assert: None,
            mock: None,
            signal: None,
        }
    }

    fn skipped(&self) -> bool {
        matches!(self.directive, Some(report::Directive::Skip(_)))
    }

    fn todo(&self) -> bool {
        matches!(self.directive, Some(report::Directive::Todo(_)))
    }
}

/// Where completed reports go: rendered here, or streamed to a `--test` coordinator.
pub(crate) enum Output {
    Render(report::Reporter),
    Stream,
}

/// One realm's harness (Node's `globalRoot` and its reporter).
#[derive(Default)]
pub(crate) struct State {
    tree: Vec<Node>,
    output: Option<Output>,
    /// Nodes whose bodies are running synchronously: where `test()` and hooks register.
    building: Vec<NodeId>,
    started: Option<Instant>,
    test_prototype: Option<RootId>,
    suite_prototype: Option<RootId>,
    tracker_prototype: Option<RootId>,
    root_mock: Option<RootId>,
    only_filter: bool,
    /// Set once the event loop drained: the root stops waiting for more tests.
    draining: bool,
}

fn with_state<T>(c: &mut Context<'_>, f: impl FnOnce(&mut State) -> T) -> T {
    let shared = c.host_mut().shared_state();
    let mut shared = shared.borrow_mut();
    f(&mut shared.node_test)
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

fn function(c: &mut Context<'_>, operation: &str, name: &str, data: RootId) -> Result<RootId, RootedError> {
    c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)
}

fn accessor(c: &mut Context<'_>, target: RootId, name: &str, getter: RootId) -> Result<(), RootedError> {
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "get", getter)?;
    let yes = c.boolean(true);
    put(c, descriptor, "configurable", yes)?;
    let key = c.string_rooted(name);
    c.define_property_descriptor_rooted(target, key, descriptor)?;
    Ok(())
}

fn node_of(c: &mut Context<'_>, object: RootId) -> Result<Option<NodeId>, RootedError> {
    Ok(super::private_slots::read(c, object, NODE_SLOT)?
        .and_then(|slot| c.rooted_value(slot))
        .and_then(|value| value.as_number())
        .map(|value| value as NodeId))
}

fn data_text(c: &mut Context<'_>) -> Result<String, RootedError> {
    let data = c.host_function_data()?;
    c.to_string(data)
}

fn is_callable(c: &mut Context<'_>, value: Option<RootId>) -> Result<Option<RootId>, RootedError> {
    match value {
        Some(value) if c.is_callable_rooted(value)? => Ok(Some(value)),
        _ => Ok(None),
    }
}

/// The exports of `node:test`: the `test` function carrying every other export.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let mut factories = Vec::with_capacity(FACTORIES.len());
    for (tag, kind, _) in FACTORIES {
        let data = c.string_rooted(tag);
        let name = match (kind, tag.split_once('.')) {
            (_, Some((_, variant))) => variant.to_owned(),
            (Kind::Suite, None) => "suite".to_owned(),
            _ => "test".to_owned(),
        };
        factories.push((tag, function(c, "nodeTestRegister", &name, data)?));
    }
    let factory = |tag: &str| factories.iter().find(|(name, _)| *name == tag).map(|(_, f)| *f);
    let test = factory("test").ok_or_else(|| RootedError::host("node:test factory missing"))?;
    let suite = factory("suite").ok_or_else(|| RootedError::host("node:test factory missing"))?;
    for base in [test, suite] {
        let prefix = if base == test { "test" } else { "suite" };
        for variant in ["skip", "todo", "only"] {
            let value = factory(&format!("{prefix}.{variant}")).ok_or_else(|| RootedError::host("node:test factory missing"))?;
            put(c, base, variant, value)?;
        }
    }
    let undefined = c.undefined();
    for name in ["after", "afterEach", "before", "beforeEach"] {
        let data = c.string_rooted(name);
        let hook = function(c, "nodeTestHook", name, data)?;
        put(c, test, name, hook)?;
    }
    let run = function(c, "nodeTestRun", "run", undefined)?;
    for (name, value) in [("describe", suite), ("it", test), ("run", run), ("suite", suite), ("test", test)] {
        put(c, test, name, value)?;
    }
    let getter = function(c, "nodeTestRootMock", "get mock", undefined)?;
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "get", getter)?;
    let yes = c.boolean(true);
    put(c, descriptor, "enumerable", yes)?;
    put(c, descriptor, "configurable", yes)?;
    let key = c.string_rooted("mock");
    c.define_property_descriptor_rooted(test, key, descriptor)?;
    Ok(test)
}

/// Node's `lazyBootstrapRoot`: the root test, its reporter and its `beforeExit` finalizer.
fn bootstrap(c: &mut Context<'_>) -> Result<(), RootedError> {
    if with_state(c, |state| !state.tree.is_empty()) {
        return Ok(());
    }
    let child = std::env::var(cli::CHILD_CONTEXT_VARIABLE).is_ok_and(|value| value == cli::CHILD_CONTEXT);
    let (requested, only_filter) = {
        let shared = c.host_mut().shared_state();
        let shared = shared.borrow();
        let options = &shared.cli_options;
        (
            options.was_provided("--test-reporter").then(|| options.string_value("--test-reporter").unwrap_or_default().to_owned()),
            options.enabled("--test-only"),
        )
    };
    let output = if child {
        Output::Stream
    } else {
        let style = report::Style::select(requested.as_deref(), crate::modules::tty_state::is_terminal_fd(1)).map_err(
            |name| RootedError::host(format!("node:test reporter '{name}' is not supported")),
        )?;
        Output::Render(report::Reporter::new(style, current_directory()))
    };
    let mut root = Node::new(None, "<root>".to_owned(), Kind::Root, 0);
    let process = process(c)?;
    let argv = get(c, process, "argv")?;
    let entry = get(c, argv, "1")?;
    root.location = c.string_text(entry)?.map(|file| report::Location { file, line: 1, column: 1 });
    with_state(c, |state| {
        state.tree.push(root);
        state.output = Some(output);
        state.started = Some(Instant::now());
        state.only_filter = only_filter;
        state.tree[ROOT].run_only = only_filter;
    });
    let on = get(c, process, "on")?;
    for (event, operation, data) in [
        ("uncaughtException", "nodeTestProcessError", "uncaughtException"),
        ("unhandledRejection", "nodeTestProcessError", "unhandledRejection"),
        ("beforeExit", "nodeTestBeforeExit", ""),
    ] {
        let event = c.string_rooted(event);
        let data = c.string_rooted(data);
        let handler = function(c, operation, "", data)?;
        c.call_rooted(on, process, &[event, handler])?;
    }
    Ok(())
}

fn current_directory() -> String {
    std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
}

fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process module is not initialized"))
}

/// `test([name][, options][, fn])` and its `describe`/`skip`/`todo`/`only` forms.
pub(crate) fn register(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let tag = data_text(c)?;
    let (_, kind, forced) = FACTORIES
        .into_iter()
        .find(|(name, _, _)| *name == tag)
        .ok_or_else(|| RootedError::host("unknown node:test factory"))?;
    bootstrap(c)?;
    let parent = with_state(c, |state| state.building.last().copied()).unwrap_or(ROOT);
    create(c, parent, kind, forced, args)
}

/// `createSubtest`: a node under `parent` from `(name, options, fn)`.
fn create(c: &mut Context<'_>, parent: NodeId, kind: Kind, forced: Override, args: &[RootId]) -> Result<RootId, RootedError> {
    let location = c.source_stack_frames(1).into_iter().next().map(|frame| report::Location {
        file: frame.script_name.strip_prefix("file://").map(str::to_owned).unwrap_or(frame.script_name),
        line: frame.line_number,
        column: frame.column_number,
    });
    let argument = |index: usize| args.get(index).copied();
    let (name, options, body) = match (argument(0), argument(1), argument(2)) {
        (Some(name), _, _) if c.is_callable_rooted(name)? => (None, None, Some(name)),
        (Some(options), body, _) if c.is_object_rooted(options)? => (None, Some(options), body),
        (name, Some(body), _) if c.is_callable_rooted(body)? => (name, None, Some(body)),
        (name, options, body) => (name, options, body),
    };
    let name = match name {
        Some(name) => c.string_text(name)?,
        None => None,
    };
    let options = match options {
        Some(options) if c.is_object_rooted(options)? => Some(options),
        _ => None,
    };
    let body = is_callable(c, body)?;
    let name = match name.filter(|name| !name.is_empty()) {
        Some(name) => name,
        None => match body {
            Some(body) => {
                let value = get(c, body, "name")?;
                c.string_text(value)?.filter(|name| !name.is_empty()).unwrap_or_else(|| "<anonymous>".to_owned())
            }
            None => "<anonymous>".to_owned(),
        },
    };
    let option = |c: &mut Context<'_>, key: &str| -> Result<Option<RootId>, RootedError> {
        match options {
            Some(options) => {
                let value = get(c, options, key)?;
                Ok((!c.rooted_value(value).is_some_and(|value| value.is_undefined())).then_some(value))
            }
            None => Ok(None),
        }
    };
    let directive_option = |c: &mut Context<'_>, value: Option<RootId>| -> Result<Option<String>, RootedError> {
        match value {
            Some(value) if c.truthy_rooted(value)? => Ok(Some(c.string_text(value)?.unwrap_or_default())),
            _ => Ok(None),
        }
    };
    let skip = option(c, "skip")?;
    let skip = directive_option(c, skip)?.or((forced == Override::Skip).then(String::new));
    let todo = option(c, "todo")?;
    let todo = directive_option(c, todo)?.or((forced == Override::Todo).then(String::new));
    let only = option(c, "only")?;
    let only = forced == Override::Only || match only {
        Some(only) => c.truthy_rooted(only)?,
        None => false,
    };
    let timeout = option(c, "timeout")?;
    let timeout = match timeout {
        Some(timeout) => match c.rooted_value(timeout).and_then(|value| value.as_number()) {
            Some(milliseconds) if (0.0..=TIMEOUT_MAX_MS).contains(&milliseconds) || milliseconds == f64::INFINITY => Some(milliseconds),
            Some(_) => {
                let received = super::util_shared_vm::range_error_received(c, timeout)?;
                let message = format!("The value of \"options.timeout\" is out of range. It must be >= 0 && <= {TIMEOUT_MAX_MS}. Received {received}");
                let error = c.range_error_rooted(&message)?;
                super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
                return Err(c.throw(error));
            }
            None => return Err(super::util_shared_vm::argument_type_error(c, timeout, "options.timeout", "number")?),
        },
        None => None,
    }
    .filter(|timeout| timeout.is_finite());
    let plan = option(c, "plan")?;
    let plan = match plan {
        Some(plan) => c.rooted_value(plan).and_then(|value| value.as_number()).map(|count| count as usize),
        None => None,
    };
    let body = match body {
        Some(body) if skip.is_none() => Some(c.retain(body)?),
        _ => None,
    };
    let (parent_kind, parent_todo, parent_timeout, parent_run_only, nesting, only_filter) = with_state(c, |state| {
        let node = &state.tree[parent];
        let nesting = if node.kind == Kind::Root { 0 } else { node.nesting + 1 };
        (node.kind, node.todo(), node.timeout, node.run_only, nesting, state.only_filter)
    });
    let mut node = Node::new(Some(parent), name, kind, nesting);
    node.location = location;
    node.body = body;
    node.plan = plan;
    node.timeout = timeout.or(parent_timeout);
    node.directive = match (skip, todo) {
        (Some(message), _) => Some(report::Directive::Skip(message)),
        (None, Some(message)) => Some(report::Directive::Todo(message)),
        (None, None) if parent_todo => Some(report::Directive::Todo(String::new())),
        (None, None) => None,
    };
    node.only = only;
    if (only || parent_run_only) && !only_filter {
        node.diagnostics.push(ONLY_WARNING.to_owned());
    }
    if only && only_filter {
        with_state(c, |state| {
            state.tree[parent].run_only = true;
            if parent == ROOT || state.tree[parent].started.is_none() {
                let mut ancestor = Some(parent);
                while let Some(current) = ancestor.filter(|current| !state.tree[*current].has_only) {
                    state.tree[current].has_only = true;
                    ancestor = state.tree[current].parent;
                }
            }
        });
    }
    let completion = c.pending_promise_rooted()?;
    node.completion = Some(c.retain(completion)?);
    let id = with_state(c, |state| {
        let id = state.tree.len();
        state.tree.push(node);
        state.tree[parent].children.push(id);
        id
    });
    if kind == Kind::Suite {
        run::build_suite(c, id)?;
    }
    match parent_kind {
        Kind::Suite => {
            let undefined = c.undefined();
            let resolved = c.pending_promise_rooted()?;
            c.resolve_promise_rooted(resolved, undefined)?;
            Ok(resolved)
        }
        Kind::Root => {
            run::schedule(c, ROOT)?;
            Ok(completion)
        }
        Kind::Test => {
            run::pump(c, parent)?;
            Ok(completion)
        }
    }
}

/// A global `before`/`after`/`beforeEach`/`afterEach`: a hook of the building node or root.
pub(crate) fn global_hook(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    bootstrap(c)?;
    let owner = with_state(c, |state| state.building.last().copied()).unwrap_or(ROOT);
    add_hook(c, owner, args)
}

fn add_hook(c: &mut Context<'_>, owner: NodeId, args: &[RootId]) -> Result<RootId, RootedError> {
    let name = data_text(c)?;
    let kind = HOOKS
        .iter()
        .find(|(hook, _)| *hook == name)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| RootedError::host("unknown node:test hook"))?;
    let undefined = c.undefined();
    let candidate = args.first().copied().unwrap_or(undefined);
    let Some(hook) = is_callable(c, Some(candidate))? else {
        return Err(super::util_shared_vm::argument_type_error(c, candidate, "fn", "function")?);
    };
    let started = with_state(c, |state| {
        let node = &state.tree[owner];
        node.kind == Kind::Root || node.started.is_some() && node.kind == Kind::Test
    });
    if kind == HookKind::Before && started {
        run::run_before_now(c, owner, hook)?;
        return Ok(c.undefined());
    }
    let hook = c.retain(hook)?;
    with_state(c, |state| state.tree[owner].hooks.list_mut(kind).push(hook));
    Ok(c.undefined())
}

/// The TestContext (or SuiteContext) of a node, made on first use.
fn context(c: &mut Context<'_>, id: NodeId) -> Result<RootId, RootedError> {
    if let Some(context) = with_state(c, |state| state.tree[id].context) {
        return Ok(context);
    }
    let kind = with_state(c, |state| state.tree[id].kind);
    let prototype = if kind == Kind::Suite { suite_prototype(c)? } else { test_prototype(c)? };
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let slot = c.number(id as f64);
    super::private_slots::write(c, object, NODE_SLOT, slot)?;
    let object = c.retain(object)?;
    with_state(c, |state| state.tree[id].context = Some(object));
    Ok(object)
}

/// The operation of a context accessor; every other member is a method.
const GETTER: &str = "nodeTestContextGet";
/// A context class member: (name, operation, operation data), in Node's order.
type Member = (&'static str, &'static str, &'static str);
const SUITE_MEMBERS: [Member; 4] = [
    ("signal", GETTER, "signal"),
    ("name", GETTER, "name"),
    ("filePath", GETTER, "filePath"),
    ("fullName", GETTER, "fullName"),
];
const TEST_MEMBERS: [Member; 19] = [
    ("signal", GETTER, "signal"),
    ("name", GETTER, "name"),
    ("filePath", GETTER, "filePath"),
    ("fullName", GETTER, "fullName"),
    ("error", GETTER, "error"),
    ("passed", GETTER, "passed"),
    ("diagnostic", "nodeTestDiagnostic", ""),
    ("plan", "nodeTestPlan", ""),
    ("assert", GETTER, "assert"),
    ("mock", GETTER, "mock"),
    ("runOnly", "nodeTestRunOnly", ""),
    ("skip", "nodeTestDirective", "skip"),
    ("todo", "nodeTestDirective", "todo"),
    ("test", "nodeTestSubtest", ""),
    ("before", "nodeTestContextHook", "before"),
    ("after", "nodeTestContextHook", "after"),
    ("beforeEach", "nodeTestContextHook", "beforeEach"),
    ("afterEach", "nodeTestContextHook", "afterEach"),
    ("waitFor", "nodeTestWaitFor", ""),
];

fn class_prototype(c: &mut Context<'_>, name: &str, members: &[Member]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let constructor = function(c, "nodeTestIllegalConstructor", name, undefined)?;
    let prototype = c.object_rooted()?;
    let key = c.string_rooted("constructor");
    c.define_data_property_rooted(prototype, key, constructor, true, false, true)?;
    let key = c.string_rooted("prototype");
    c.define_data_property_rooted(constructor, key, prototype, false, false, false)?;
    for (name, operation, tag) in members {
        let data = c.string_rooted(tag);
        if *operation == GETTER {
            let getter = function(c, operation, &format!("get {name}"), data)?;
            accessor(c, prototype, name, getter)?;
        } else {
            let method = function(c, operation, name, data)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(prototype, key, method, true, false, true)?;
        }
    }
    c.retain(prototype)
}

fn test_prototype(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(prototype) = with_state(c, |state| state.test_prototype) {
        return Ok(prototype);
    }
    let prototype = class_prototype(c, "TestContext", &TEST_MEMBERS)?;
    with_state(c, |state| state.test_prototype = Some(prototype));
    Ok(prototype)
}

fn suite_prototype(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(prototype) = with_state(c, |state| state.suite_prototype) {
        return Ok(prototype);
    }
    let prototype = class_prototype(c, "SuiteContext", &SUITE_MEMBERS)?;
    with_state(c, |state| state.suite_prototype = Some(prototype));
    Ok(prototype)
}

pub(crate) fn illegal_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let error = c.type_error_rooted("Illegal constructor")?;
    super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_ILLEGAL_CONSTRUCTOR")?;
    Err(c.throw(error))
}

fn receiver(c: &mut Context<'_>, this: RootId) -> Result<NodeId, RootedError> {
    match node_of(c, this)? {
        Some(id) => Ok(id),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
            Err(c.throw(error))
        }
    }
}

fn full_name(state: &State, id: NodeId) -> String {
    let mut names = Vec::new();
    let mut current = Some(id);
    while let Some(node) = current.map(|id| &state.tree[id]).filter(|node| node.kind != Kind::Root) {
        names.push(node.name.as_str());
        current = node.parent;
    }
    names.reverse();
    names.join(" > ")
}

pub(crate) fn context_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let field = data_text(c)?;
    let id = receiver(c, this)?;
    match field.as_str() {
        "name" => {
            let name = with_state(c, |state| state.tree[id].name.clone());
            Ok(c.string_rooted(&name))
        }
        "fullName" => {
            let name = with_state(c, |state| full_name(state, id));
            Ok(c.string_rooted(&name))
        }
        "filePath" => {
            let file = with_state(c, |state| state.tree[ROOT].location.as_ref().map(|location| location.file.clone()));
            Ok(match file {
                Some(file) => c.string_rooted(&file),
                None => c.undefined(),
            })
        }
        "passed" => {
            let passed = with_state(c, |state| state.tree[id].failure.is_none());
            Ok(c.boolean(passed))
        }
        "error" => Ok(c.null()),
        "signal" => signal(c, id),
        "assert" => assert(c, id),
        "mock" => mock::test_tracker(c, id),
        _ => Ok(c.undefined()),
    }
}

/// The node's AbortSignal, from an AbortController made on first read.
fn signal(c: &mut Context<'_>, id: NodeId) -> Result<RootId, RootedError> {
    if let Some(signal) = with_state(c, |state| state.tree[id].signal) {
        return Ok(signal);
    }
    let global = c.global_root()?;
    let controller = get(c, global, "AbortController")?;
    let controller = c.construct_rooted(controller, controller, &[])?;
    let signal = get(c, controller, "signal")?;
    let signal = c.retain(signal)?;
    with_state(c, |state| state.tree[id].signal = Some(signal));
    Ok(signal)
}

/// The assertion methods `t.assert` binds to the test's plan.
const PLANNED_ASSERTIONS: [&str; 18] = [
    "deepEqual",
    "deepStrictEqual",
    "doesNotMatch",
    "doesNotReject",
    "doesNotThrow",
    "equal",
    "fail",
    "ifError",
    "match",
    "notDeepEqual",
    "notDeepStrictEqual",
    "notEqual",
    "notStrictEqual",
    "partialDeepStrictEqual",
    "rejects",
    "strictEqual",
    "throws",
    "ok",
];

fn assert(c: &mut Context<'_>, id: NodeId) -> Result<RootId, RootedError> {
    if let Some(assert) = with_state(c, |state| state.tree[id].assert) {
        return Ok(assert);
    }
    let module = super::assert_shared_vm::module(c)?;
    let object = c.object_rooted()?;
    let node = c.number(id as f64);
    for name in PLANNED_ASSERTIONS {
        let method = get(c, module, name)?;
        if !c.is_callable_rooted(method)? {
            continue;
        }
        let data = c.array_rooted(&[method, node])?;
        let wrapper = function(c, "nodeTestAssert", name, data)?;
        put(c, object, name, wrapper)?;
    }
    let object = c.retain(object)?;
    with_state(c, |state| state.tree[id].assert = Some(object));
    Ok(object)
}

pub(crate) fn planned_assert(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let method = get(c, data, "0")?;
    let node = get(c, data, "1")?;
    let id = c.rooted_value(node).and_then(|value| value.as_number()).unwrap_or_default() as NodeId;
    with_state(c, |state| state.tree[id].assertions += 1);
    let undefined = c.undefined();
    c.call_rooted(method, undefined, args)
}

pub(crate) fn diagnostic(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver(c, this)?;
    let message = args.first().copied().unwrap_or_else(|| c.undefined());
    let message = c.to_string(message)?;
    with_state(c, |state| state.tree[id].diagnostics.push(message));
    Ok(c.undefined())
}

pub(crate) fn plan(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver(c, this)?;
    let count = args.first().copied().unwrap_or_else(|| c.undefined());
    let maximum = c.number(f64::from(u32::MAX));
    let count = super::util_shared_vm::validate_integer(c, count, "count", 0.0, maximum)?;
    if with_state(c, |state| state.tree[id].plan.is_some()) {
        let error = c.error_rooted("cannot set plan more than once")?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_TEST_FAILURE")?;
        return Err(c.throw(error));
    }
    with_state(c, |state| state.tree[id].plan = Some(count as usize));
    Ok(c.undefined())
}

pub(crate) fn run_only(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver(c, this)?;
    let enabled = match args.first().copied() {
        Some(value) => c.truthy_rooted(value)?,
        None => false,
    };
    with_state(c, |state| state.tree[id].run_only = enabled);
    Ok(c.undefined())
}

pub(crate) fn directive(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let tag = data_text(c)?;
    let id = receiver(c, this)?;
    let message = match args.first().copied() {
        Some(message) => c.string_text(message)?.unwrap_or_default(),
        None => String::new(),
    };
    let directive = if tag == "skip" { report::Directive::Skip(message) } else { report::Directive::Todo(message) };
    with_state(c, |state| state.tree[id].directive = Some(directive));
    Ok(c.undefined())
}

pub(crate) fn subtest(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver(c, this)?;
    create(c, id, Kind::Test, Override::None, args)
}

pub(crate) fn context_hook(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver(c, this)?;
    add_hook(c, id, args)
}

/// The module's `mock` getter: the process-wide MockTracker.
pub(crate) fn root_mock(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(tracker) = with_state(c, |state| state.root_mock) {
        return Ok(tracker);
    }
    let tracker = mock::tracker(c)?;
    with_state(c, |state| state.root_mock = Some(tracker));
    Ok(tracker)
}

/// `run({ files })`: the files' reports, as `--test` collects them, on an object-mode stream.
pub(crate) fn run_files(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mut files = Vec::new();
    if let Some(options) = args.first().copied().filter(|options| c.is_object_rooted(*options).unwrap_or(false)) {
        let list = get(c, options, "files")?;
        if c.is_object_rooted(list)? {
            for file in c.array_like_values_rooted(list)? {
                files.push(c.to_string(file)?);
            }
        }
    }
    let runs = cli::run_files(&cli::resolve_files(&files));
    let stream = crate::host::shared_vm::commonjs::stream_module(c)?;
    let readable = get(c, stream, "Readable")?;
    let options = c.object_rooted()?;
    let yes = c.boolean(true);
    put(c, options, "objectMode", yes)?;
    let undefined = c.undefined();
    let read = function(c, "nodeTestRunRead", "read", undefined)?;
    put(c, options, "read", read)?;
    let readable = c.construct_rooted(readable, readable, &[options])?;
    let mut counters = report::Counters::default();
    let mut events = Vec::new();
    for run in &runs {
        for test in &run.reports {
            counters.count(test);
            run::events(c, test, &run.file, &mut events)?;
        }
    }
    events.push(run::summary_event(c, &counters)?);
    let events = c.array_rooted(&events)?;
    let data = c.array_rooted(&[readable, events])?;
    let deliver = function(c, "nodeTestRunDeliver", "", data)?;
    let global = c.global_root()?;
    let set_immediate = get(c, global, "setImmediate")?;
    c.call_rooted(set_immediate, global, &[deliver])?;
    Ok(readable)
}

pub(crate) fn run_read(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// Emit each event under its type (TestsStream's `#emit`), push it, then end the stream.
pub(crate) fn run_deliver(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let stream = get(c, data, "0")?;
    let events = get(c, data, "1")?;
    let emit = get(c, stream, "emit")?;
    let push = get(c, stream, "push")?;
    for event in c.array_like_values_rooted(events)? {
        let kind = get(c, event, "type")?;
        let payload = get(c, event, "data")?;
        c.call_rooted(emit, stream, &[kind, payload])?;
        c.call_rooted(push, stream, &[event])?;
    }
    let end = c.null();
    c.call_rooted(push, stream, &[end])?;
    Ok(c.undefined())
}
