use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionMode {
    NotExecuted,
    InlineStencil,
    KernelExit,
    Interpreted,
}

impl ExecutionMode {
    fn name(self) -> &'static str {
        match self {
            Self::NotExecuted => "NOT_EXECUTED",
            Self::InlineStencil => "INLINE_STENCIL",
            Self::KernelExit => "KERNEL_EXIT",
            Self::Interpreted => "INTERPRETED",
        }
    }
}

#[derive(Debug)]
struct SourceFile {
    path: PathBuf,
    text: String,
    line_starts: Vec<usize>,
}

#[derive(Debug)]
struct LineCoverage {
    text: String,
    executable: bool,
    mode: ExecutionMode,
    executions: u64,
    operations: BTreeSet<String>,
}

#[derive(Default)]
pub struct Coverage {
    enabled: bool,
    sources: Vec<SourceFile>,
    lines: BTreeMap<(usize, usize), LineCoverage>,
}

impl Coverage {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn location(&self, source_id: usize, byte_offset: u32) -> Option<(PathBuf, usize)> {
        let source = self.sources.get(source_id)?;
        let offset = (byte_offset as usize).min(source.text.len());
        let line = source
            .line_starts
            .partition_point(|start| *start <= offset)
            .max(1);
        Some((source.path.clone(), line))
    }

    pub fn register_source(&mut self, path: &Path, text: &str) -> usize {
        let source_id = self.sources.len();
        let mut line_starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        let mut in_block_comment = false;
        for (index, line) in text.lines().enumerate() {
            let executable = looks_executable(line, &mut in_block_comment);
            if executable {
                self.lines.insert(
                    (source_id, index + 1),
                    LineCoverage {
                        text: line.to_string(),
                        executable,
                        mode: ExecutionMode::NotExecuted,
                        executions: 0,
                        operations: BTreeSet::new(),
                    },
                );
            }
        }
        self.sources.push(SourceFile {
            path: path.to_path_buf(),
            text: text.to_string(),
            line_starts,
        });
        source_id
    }

    pub fn hit(&mut self, source_id: usize, byte_offset: u32, operation: &str) {
        self.mark(
            source_id,
            byte_offset,
            operation,
            ExecutionMode::Interpreted,
        );
    }

    pub fn mark(
        &mut self,
        source_id: usize,
        byte_offset: u32,
        operation: &str,
        mode: ExecutionMode,
    ) {
        if !self.enabled {
            return;
        }
        let Some(source) = self.sources.get(source_id) else {
            return;
        };
        let offset = (byte_offset as usize).min(source.text.len());
        let line = source.line_starts.partition_point(|start| *start <= offset);
        let entry = self
            .lines
            .entry((source_id, line.max(1)))
            .or_insert_with(|| {
                let text = source
                    .text
                    .lines()
                    .nth(line.saturating_sub(1))
                    .unwrap_or("");
                LineCoverage {
                    text: text.to_string(),
                    executable: true,
                    mode: ExecutionMode::NotExecuted,
                    executions: 0,
                    operations: BTreeSet::new(),
                }
            });
        entry.mode = entry.mode.max(mode);
        entry.executions = entry.executions.saturating_add(1);
        entry.operations.insert(operation.to_string());
    }

    pub fn write(&self, jsonl_path: &Path) -> io::Result<()> {
        if let Some(parent) = jsonl_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let markdown_path = jsonl_path.with_extension("md");
        fs::write(jsonl_path, self.jsonl())?;
        fs::write(markdown_path, self.markdown())
    }

    fn jsonl(&self) -> String {
        let mut out = String::new();
        for ((source_id, line), coverage) in &self.lines {
            let source = &self.sources[*source_id];
            let operations = coverage
                .operations
                .iter()
                .map(|item| format!("\"{}\"", escape(item)))
                .collect::<Vec<_>>()
                .join(",");
            let selected = selected_treatment(&coverage.text);
            let alternatives = alternative_treatments(&coverage.text);
            out.push_str(&format!(
                "{{\"source\":\"{}\",\"line\":{},\"column\":1,\"source_text\":\"{}\",\"execution_mode\":\"{}\",\"execution_count\":{},\"executable\":{},\"operations\":[{}],\"selected_treatment\":\"{}\",\"alternative_treatments\":\"{}\"}}\n",
                escape(&source.path.display().to_string()),
                line,
                escape(&coverage.text),
                coverage.mode.name(),
                coverage.executions,
                coverage.executable,
                operations,
                escape(selected),
                escape(alternatives),
            ));
        }
        out
    }

    fn markdown(&self) -> String {
        let mut out = String::from(
            "# Stencil coverage gaps\n\nEvery executable source line is listed. KERNEL_EXIT means semantics ran in a shared Rust slow kernel; INLINE_STENCIL means semantics ran in copied machine code. INTERPRETED is diagnostic-only and NOT_EXECUTED means the line was not reached.\n\n",
        );
        for ((source_id, line), coverage) in &self.lines {
            if matches!(
                coverage.mode,
                ExecutionMode::InlineStencil | ExecutionMode::KernelExit
            ) {
                continue;
            }
            let source = &self.sources[*source_id];
            out.push_str(&format!(
                "- `{}:{}` `{}` — **{}**, executions: {}. Selected: {} Alternatives: {}\n",
                source.path.display(),
                line,
                coverage.text.trim().replace('`', "\\`"),
                coverage.mode.name(),
                coverage.executions,
                selected_treatment(&coverage.text),
                alternative_treatments(&coverage.text),
            ));
        }
        out
    }
}

fn looks_executable(line: &str, in_block_comment: &mut bool) -> bool {
    let mut text = line.trim();
    if *in_block_comment {
        if let Some(end) = text.find("*/") {
            *in_block_comment = false;
            text = text[end + 2..].trim();
        } else {
            return false;
        }
    }
    if text.starts_with("/*") {
        if let Some(end) = text.find("*/") {
            text = text[end + 2..].trim();
        } else {
            *in_block_comment = true;
            return false;
        }
    }
    !text.is_empty() && !text.starts_with("//") && !matches!(text, "{" | "}" | "};")
}

fn selected_treatment(line: &str) -> &'static str {
    let text = line.trim();
    if text.starts_with("function ") || text.contains("= function") {
        "CreateClosure helper stencil with patched function metadata"
    } else if text.starts_with("if ") || text.starts_with("if(") || text.starts_with("switch ") {
        "native comparison/branch stencil with symbolic join labels"
    } else if text.starts_with("for ")
        || text.starts_with("for(")
        || text.starts_with("while ")
        || text.starts_with("while(")
        || text.starts_with("do ")
    {
        "native loop-region stencil with patched backedge"
    } else if text.contains("new ") {
        "Construct helper stencil returning to the native CFG"
    } else if text.contains('[') && (text.contains("]=") || text.contains("] =")) {
        "computed-element store helper stencil"
    } else if text.contains('[') {
        "computed-element load helper stencil"
    } else if text.contains('.') && text.contains('(') {
        "CallWithReceiver helper stencil; later monomorphic call IC"
    } else if text.contains('.') && text.contains('=') {
        "static-property store helper stencil; later shape/offset IC"
    } else if text.contains('.') {
        "static-property load helper stencil; later shape/offset IC"
    } else if text.starts_with("return") {
        "native return/epilogue stencil"
    } else if text.starts_with("throw") || text.starts_with("try") || text.starts_with("catch") {
        "exception helper stencil with native exceptional edge"
    } else {
        "lower to register bytecode; use guarded int32/f64 stencils and semantic helpers"
    }
}

fn alternative_treatments(line: &str) -> &'static str {
    if line.contains('.') || line.contains('[') {
        "guarded inline IC; generic property helper stencil; reject compilation"
    } else if line.contains('(') {
        "specialized call/construct stencil; generic call helper stencil; reject compilation"
    } else {
        "fused block stencil; primitive opcode stencil; reject compilation"
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_every_executable_line_and_runtime_hit() {
        let mut coverage = Coverage::enabled();
        let source = "// comment\nfunction f(o) {\n  return o.x;\n}\n";
        let id = coverage.register_source(Path::new("fixture.js"), source);
        coverage.hit(id, source.find("return").unwrap() as u32, "ReturnStatement");
        let json = coverage.jsonl();
        assert!(json.contains("fixture.js"));
        assert!(json.contains("return o.x"));
        assert!(json.contains("INTERPRETED"));
        assert!(!json.contains("// comment"));
    }
}
