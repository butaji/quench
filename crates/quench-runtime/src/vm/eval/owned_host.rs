//! Host Script source the embedding owns, compiled and evaluated without a copy of it: the
//! program keeps the string as its text (`SourceText::Owned`).
use super::*;

impl Vm {
    /// Evaluates host source the caller owns as a Script: the classic, entry or TypeScript form,
    /// as the matching `evaluate_embedding_*_with_origin` evaluates a `&str`.
    pub(crate) fn evaluate_embedding_owned_script_with_origin(
        &mut self, source: std::rc::Rc<String>, name: &str, origin: (u32, u32), keep_records: bool, typescript: bool,
    ) -> Result<Value, JsError> {
        let prepared = self.prepare_embedding_owned_script(source, name, Some(origin), keep_records, typescript)?;
        self.evaluate_embedding_prepared_script(&prepared)
    }

    /// `prepare_embedding_specialized_script` for host source the caller owns.
    pub(crate) fn prepare_embedding_owned_script(
        &mut self, source: std::rc::Rc<String>, name: &str, origin: Option<(u32, u32)>, keep_records: bool, typescript: bool,
    ) -> Result<crate::PreparedScript, JsError> {
        let provided = crate::bytecode::SourceText::Owned(source.clone());
        self.prepare_embedding_specialized_script_retaining(
            source.as_str(), name, origin, None, keep_records, typescript, Some(provided),
        )
    }

    /// `compile_embedding_host_script` for host source the caller owns.
    pub(crate) fn compile_embedding_owned_host_script(
        &mut self, source: std::rc::Rc<String>, name: &str, origin: (u32, u32),
    ) -> Result<crate::CompiledScript, JsError> {
        let prepared = self.prepare_embedding_owned_script(source, name, Some(origin), false, false)?;
        self.register_embedding_compiled_script(prepared)
    }
}
