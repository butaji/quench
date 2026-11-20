//! `fs.glob`, `fs.globSync` and `fs.promises.glob` (`internal/fs/glob.js`): the paths under
//! `cwd` that match any of the patterns and none of the exclusions.

use super::api::{Call, Reply};
use crate::modules::path_algorithms::posix;
use crate::modules::stream_shared_vm::{get, is_undefined, Context, Outcome};
use quench_runtime::{RootId, RootedError};
use std::path::PathBuf;

/// A compiled pattern: what a path relative to the walk's root must match.
struct Pattern {
    expression: regex::Regex,
    /// `a/**` also names `a` itself; `**` names the root.
    prefix: Option<regex::Regex>,
    /// A pattern ending in `/` matches only directories.
    directories_only: bool,
    /// Entries whose name starts with `.` match only a pattern naming one.
    dot: bool,
    /// The literal leading segments, where the walk starts.
    base: String,
    /// The segments below `base` a match may have, unless `**` reaches any depth.
    depth: Option<usize>,
}

const GLOB_CHARACTERS: &[char] = &['*', '?', '[', '{'];
const GLOBSTAR: &str = "**";

/// `{a,b}` alternatives, expanded (innermost first) into separate patterns.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let mut depth = 0;
    let mut close = None;
    let mut commas = Vec::new();
    for (index, character) in pattern[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + index);
                    break;
                }
            }
            ',' if depth == 1 => commas.push(open + index),
            _ => {}
        }
    }
    let Some(close) = close.filter(|_| !commas.is_empty()) else {
        return vec![pattern.to_owned()];
    };
    let mut bounds = vec![open];
    bounds.extend(&commas);
    bounds.push(close);
    let (before, after) = (&pattern[..open], &pattern[close + 1..]);
    bounds
        .windows(2)
        .flat_map(|pair| expand_braces(&format!("{before}{}{after}", &pattern[pair[0] + 1..pair[1]])))
        .collect()
}

fn compile(source: &str) -> Result<regex::Regex, RootedError> {
    regex::Regex::new(&crate::modules::path_shared_vm::glob_regex_source(source)).map_err(|error| RootedError::host(error.to_string()))
}

impl Pattern {
    fn new(pattern: &str) -> Result<Vec<Self>, RootedError> {
        expand_braces(pattern).iter().map(|expanded| Self::single(expanded)).collect()
    }

    fn single(pattern: &str) -> Result<Self, RootedError> {
        let directories_only = pattern.ends_with('/') && pattern.len() > 1;
        let normalized = posix::normalize_str(pattern);
        let normalized = normalized.trim_end_matches('/');
        let normalized = if normalized.is_empty() { "/" } else { normalized };
        let segments: Vec<&str> = normalized.split('/').collect();
        let literal = segments.iter().take_while(|segment| !segment.contains(GLOB_CHARACTERS)).count();
        // The last segment is what the walk lists, so at least it is matched.
        let literal = literal.min(segments.len() - 1);
        let base = segments[..literal].join("/");
        let base = if base.is_empty() && normalized.starts_with('/') { "/".to_owned() } else { base };
        let depth = (!segments.contains(&GLOBSTAR)).then_some(segments.len() - literal);
        let prefix = if normalized == GLOBSTAR {
            Some(compile("")?)
        } else {
            normalized.strip_suffix("/**").map(compile).transpose()?
        };
        Ok(Self {
            expression: compile(normalized)?,
            prefix,
            directories_only,
            dot: segments.iter().any(|segment| segment.starts_with('.') && *segment != "." && *segment != ".."),
            base,
            depth,
        })
    }

    fn matches(&self, path: &str, directory: bool) -> bool {
        if self.directories_only && !directory {
            return false;
        }
        let hidden = path.split('/').any(|segment| segment.starts_with('.') && segment != ".");
        if hidden && !self.dot {
            return false;
        }
        let prefixed = self.prefix.as_ref().is_some_and(|prefix| prefix.is_match(path));
        // The walk's root is named only by a pattern that reaches zero segments below it.
        prefixed || (!path.is_empty() && self.expression.is_match(path))
    }
}

/// An exclusion: a function of the path (or its `Dirent`), or patterns.
enum Exclude {
    None,
    Function(RootId),
    Patterns(Vec<Pattern>),
}

/// `validateStringArrayOrFunction` reads: `patterns` is a string or an array of strings.
fn strings(c: &mut Context<'_>, value: RootId, name: &str) -> Result<Vec<String>, RootedError> {
    if let Some(text) = c.string_text(value)? {
        return Ok(vec![text]);
    }
    if !c.is_array_rooted(value)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, value, name, "string")?);
    }
    let mut texts = Vec::new();
    for (index, item) in c.array_like_values_rooted(value)?.into_iter().enumerate() {
        match c.string_text(item)? {
            Some(text) => texts.push(text),
            None => return Err(crate::modules::util_shared_vm::argument_type_error(c, item, &format!("{name}[{index}]"), "string")?),
        }
    }
    Ok(texts)
}

/// A path the walk visits: where it is on the host and the text a match reports.
struct Entry {
    io: PathBuf,
    shown: String,
    directory: bool,
    kind: i32,
    depth: usize,
}

/// The matches of `patterns` under `cwd`, each reported once in walk order.
fn walk(c: &mut Context<'_>, cwd: &str, patterns: &[Pattern], exclude: &Exclude, with_types: bool) -> Outcome {
    let mut results = Vec::new();
    let mut reported = std::collections::HashSet::new();
    let root = c.host_mut().shared_state().borrow().cwd.clone();
    for pattern in patterns {
        let base_shown = if pattern.base.is_empty() { String::new() } else { pattern.base.clone() };
        let base_io = root.resolve_io_path(std::path::Path::new(&posix::resolve_strings(&[cwd.to_owned(), base_shown.clone()], cwd)));
        let Ok(metadata) = std::fs::symlink_metadata(&base_io) else {
            continue;
        };
        let mut queue = std::collections::VecDeque::from([Entry {
            io: base_io,
            shown: base_shown,
            directory: metadata.is_dir(),
            kind: super::dir::entry_type(metadata.file_type()),
            depth: 0,
        }]);
        while let Some(entry) = queue.pop_front() {
            let shown = if entry.shown.is_empty() { "." } else { entry.shown.as_str() };
            if excluded(c, exclude, &entry, shown, with_types)? {
                continue;
            }
            let within = pattern.depth.is_none_or(|depth| entry.depth <= depth);
            if within && pattern.matches(&entry.shown, entry.directory) && reported.insert(entry.shown.clone()) {
                results.push(report(c, &entry, shown, with_types)?);
            }
            let deeper = pattern.depth.is_none_or(|depth| entry.depth < depth);
            if !entry.directory || !deeper {
                continue;
            }
            let Ok(children) = crate::modules::fs_directory_shared_vm::scan(&entry.io) else {
                continue;
            };
            for child in children {
                let name = child.name.to_string_lossy().into_owned();
                let io = entry.io.join(&child.name);
                let kind = super::dir::resolve_kind(child.kind, &io);
                let shown = if entry.shown.is_empty() { name } else { super::ops::join_shown(&entry.shown, &name) };
                queue.push_back(Entry { io, shown, directory: kind == super::UV_DIRENT_DIR, kind, depth: entry.depth + 1 });
            }
        }
    }
    c.array_rooted(&results)
}

fn report(c: &mut Context<'_>, entry: &Entry, shown: &str, with_types: bool) -> Outcome {
    if !with_types {
        return Ok(c.string_rooted(shown));
    }
    let name = shown.rsplit('/').next().unwrap_or(shown);
    let parent = posix::dirname_str(shown);
    let name = c.string_rooted(name);
    super::dir::dirent(c, name, entry.kind, &parent)
}

fn excluded(c: &mut Context<'_>, exclude: &Exclude, entry: &Entry, shown: &str, with_types: bool) -> Result<bool, RootedError> {
    match exclude {
        Exclude::None => Ok(false),
        Exclude::Patterns(patterns) => Ok(patterns.iter().any(|pattern| pattern.matches(&entry.shown, entry.directory))),
        Exclude::Function(function) => {
            let argument = report(c, entry, shown, with_types)?;
            let undefined = c.undefined();
            let verdict = c.call_rooted(*function, undefined, &[argument])?;
            c.truthy_rooted(verdict)
        }
    }
}

/// `globSync(pattern, { cwd, exclude, withFileTypes })` and its asynchronous faces.
pub(crate) fn glob(c: &mut Context<'_>, call_args: &[RootId]) -> Call {
    let sources = strings(c, call_args[0], "patterns")?;
    let options = call_args[1];
    if !is_undefined(c, options) && (!c.is_object_rooted(options)? || c.is_callable_rooted(options)?) {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let field = |c: &mut Context<'_>, name: &str| if is_undefined(c, options) { Ok(c.undefined()) } else { get(c, options, name) };
    let current = c.host_mut().shared_state().borrow().cwd.clone();
    let current = current.current_path().map_err(|error| RootedError::host(error.to_string()))?.to_string_lossy().into_owned();
    let cwd = field(c, "cwd")?;
    let cwd = if is_undefined(c, cwd) {
        current
    } else {
        let cwd = crate::modules::url_shared_vm::to_path_if_file_url(c, cwd, &[cwd])?;
        match c.string_text(cwd)? {
            Some(cwd) => posix::resolve_strings(&[cwd], &current),
            None => return Err(crate::modules::util_shared_vm::argument_type_error(c, cwd, "paths[0]", "string")?),
        }
    };
    let exclude = field(c, "exclude")?;
    let exclude = if is_undefined(c, exclude) {
        Exclude::None
    } else if c.is_callable_rooted(exclude)? {
        Exclude::Function(exclude)
    } else if c.is_array_rooted(exclude)? {
        let sources = strings(c, exclude, "options.exclude")?;
        Exclude::Patterns(sources.iter().map(|source| Pattern::new(source)).collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect())
    } else {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, exclude, "options.exclude", "of type function or string[]")?);
    };
    let with_types = field(c, "withFileTypes")?;
    let with_types = c.truthy_rooted(with_types)?;
    let patterns = sources.iter().map(|source| Pattern::new(source)).collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect::<Vec<_>>();
    Ok(walk(c, &cwd, &patterns, &exclude, with_types).map(Reply::Value))
}

/// `fsPromises.glob(pattern, options)`: an async iterator over the matches.
pub(crate) fn promise_glob(c: &mut Context<'_>, _: RootId, call_args: &[RootId]) -> Outcome {
    let mut padded = call_args.to_vec();
    padded.resize(2, c.undefined());
    let Reply::Value(matches) = glob(c, &padded)?? else {
        return Err(RootedError::host("glob reports its matches as a value"));
    };
    let state = c.null_object_rooted()?;
    crate::modules::stream_shared_vm::put(c, state, "entries", matches)?;
    let zero = c.number(0.0);
    crate::modules::stream_shared_vm::put(c, state, "index", zero)?;
    super::dir::iterate_state(c, state)
}
