from pathlib import Path
root=Path('/workspace/quench/work/prepared-realm-worktree')
def edit(name,old,new):
 p=root/name;s=p.read_text();assert old in s,(name,old);p.write_text(s.replace(old,new))
edit('crates/quench-runtime/src/heap/cell.rs','Captured(Vec<CallSiteRecord>),','Captured { records: Vec<CallSiteRecord>, source_line: Option<String> },')
edit('crates/quench-runtime/src/heap/cell.rs','Self::Captured(records) => {\n                records.capacity()', 'Self::Captured { records, source_line } => {\n                source_line.as_ref().map_or(0, String::capacity) + records.capacity()')
edit('crates/quench-runtime/src/heap/cell.rs','Some(StackData::Captured(records)) => {','Some(StackData::Captured { records, .. }) => {')
edit('crates/quench-runtime/src/vm/error.rs','Some(StackData::Captured(records)) => records,','Some(StackData::Captured { records, .. }) => records,')
edit('crates/quench-runtime/src/vm/error.rs','let mut records = Vec::with_capacity(self.frames.len().min(limit));','let mut records = Vec::with_capacity(self.frames.len().min(limit));\n        let mut source_line = None;\n        let mut source_captured = false;')
edit('crates/quench-runtime/src/vm/error.rs','if let Some((mut record, module)) = self.source_call_site_record(frame, hide_identity) {\n                Self::map_call_site_source', '''if let Some((mut record, module)) = self.source_call_site_record(frame, hide_identity) {
                if !source_captured {
                    source_captured = true;
                    if let CallSiteLocation::Source { line, .. } = &record.location {
                        source_line = residual.source.as_ref()
                            .and_then(|source| source.source_line(*line)).map(str::to_owned);
                    }
                }
                Self::map_call_site_source''')
edit('crates/quench-runtime/src/vm/error.rs','Some(_) => StackData::Captured(records),','Some(_) => StackData::Captured { records, source_line },')
edit('crates/quench-runtime/src/vm/error.rs','Some(StackData::Captured(_) | StackData::Formatted(_))','Some(StackData::Captured { .. } | StackData::Formatted(_))')
edit('crates/quench-runtime/src/api_native.rs','/// Private weak host reference, pruned by the core collector before slot reuse.','''/// Compiler-owned source for an unformatted captured error, or the current activation.
/// Positions are one-based, as in source stack frames; hosts choose their API projection.
pub struct ErrorSourcePosition {
    pub source_line: Option<String>,
    pub script_name: Option<String>,
    pub line_number: u32,
    pub column_number: u32,
}

/// Private weak host reference, pruned by the core collector before slot reuse.''')
edit('crates/quench-runtime/src/api_native.rs','    /// Compiler-owned source names of live JavaScript callers, nearest first.','''    /// Read captured source without invoking properties, getters or proxy traps.
    /// With no unformatted capture, project the current genuine source activation.
    pub fn error_source_position_rooted(
        &mut self,
        error: RootId,
    ) -> Result<Option<ErrorSourcePosition>, RootedError> {
        let result = self.vm.embedding_error_source_position(error);
        result.map_err(|error| self.error(error))
    }

    /// Compiler-owned source names of live JavaScript callers, nearest first.''')
for name in ['crates/quench-runtime/src/api.rs','crates/quench-runtime/src/lib.rs']:
 edit(name,'SourceStackFrame,','ErrorSourcePosition, SourceStackFrame,')
edit('crates/quench-runtime/src/vm/embedding.rs','    pub(crate) fn embedding_call_stack_source_names(&self) -> Vec<String> {','''    pub(crate) fn embedding_error_source_position(
        &mut self,
        error: RootId,
    ) -> Result<Option<crate::ErrorSourcePosition>, JsError> {
        let value = self.embedding_value(error)?;
        // Proxy objects deliberately do not expose their target's captured metadata.
        if let Some(crate::heap::StackData::Captured { records, source_line }) =
            self.object_data(value).and_then(crate::heap::Object::stack_data)
        {
            if let Some(record) = records.iter().find(|record| matches!(record.location, CallSiteLocation::Source { .. })) {
                let CallSiteLocation::Source { file_name, line, column } = &record.location else { unreachable!() };
                return Ok(Some(crate::ErrorSourcePosition {
                    source_line: source_line.clone(),
                    script_name: record.eval_origin.is_none().then(|| file_name.clone()),
                    line_number: *line,
                    column_number: *column,
                }));
            }
        }
        let Some((mut record, module, source_line)) = self.frames.iter().rev().find_map(|frame| {
            let (record, module) = self.source_call_site_record(frame, false)?;
            let CallSiteLocation::Source { line, .. } = &record.location else { return None };
            let source_line = self.programs.get(frame.program)?.source.as_ref()
                .and_then(|source| source.source_line(*line)).map(str::to_owned);
            Some((record, module, source_line))
        }) else { return Ok(None) };
        Self::map_call_site_source(&mut self.host, &mut record, module);
        let CallSiteLocation::Source { file_name, line, column } = record.location else { unreachable!() };
        Ok(Some(crate::ErrorSourcePosition {
            source_line,
            script_name: record.eval_origin.is_none().then_some(file_name),
            line_number: line,
            column_number: column,
        }))
    }

    pub(crate) fn embedding_call_stack_source_names(&self) -> Vec<String> {''')
