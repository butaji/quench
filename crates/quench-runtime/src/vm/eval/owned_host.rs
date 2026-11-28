//! Host Script source the embedding owns, compiled and evaluated without a copy of it: the
//! program keeps the string as its text (`SourceText::Owned`).
use super::*;

impl Vm {
    /// Evaluates host source the caller owns as a Script: the classic, entry or TypeScript form,
    /// as the matching `evaluate_embedding_*_with_origin` evaluates a `&str`. `calls_result_once` is the host's word that it
    /// calls the script's completion value once (the CommonJS loader's wrapper): the program carries it as data
    /// (`ResidualProgram::host_calls_result_once`) from before it runs, where its facts are solved.
    pub(crate) fn evaluate_embedding_owned_script_with_origin(
        &mut self, source: std::rc::Rc<String>, name: &str, origin: (u32, u32), keep_records: bool, typescript: bool,
        calls_result_once: bool,
    ) -> Result<Value, JsError> {
        let mut prepared = self.prepare_embedding_owned_script(source, name, Some(origin), keep_records, typescript)?;
        // The host's word is data of the program it runs, set on every path (compiled, kept, shared): a shared program is
        // copied before the flag is set, so the word never depends on who else holds it.
        if prepared.program.host_calls_result_once != calls_result_once {
            std::rc::Rc::make_mut(&mut prepared.program).host_calls_result_once = calls_result_once;
        }
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
