use super::NativeImageInputs;
use quench_runtime::{NativeImageSupport, native_link};
use std::{collections::BTreeSet, path::Path};
pub use native_link::NativeLinkAudit;

/// Node's SDK contract over the shared native executable publisher.
pub struct NativeSupport(native_link::NativeSupport);
impl NativeSupport {
    pub fn from_cargo_report(report: &str) -> Result<Self, String> {
        Self::from_cargo_report_with_support(report, NativeImageSupport::Static)
    }
    pub fn from_cargo_report_with_support(report: &str, support: NativeImageSupport) -> Result<Self, String> {
        native_link::NativeSupport::from_cargo_report(report, "quench_node", support).map(Self)
    }
    pub fn report(&self) -> serde_json::Value {
        let mut report = self.0.report();
        let host = report.as_object_mut().unwrap().remove("host_library").unwrap();
        report["node_library"] = host;
        report
    }
    pub fn link(&self, inputs: &NativeImageInputs, driver: &Path, executable: &Path) -> Result<BTreeSet<String>, String> {
        self.0.link(inputs, driver, executable)
    }
    pub fn link_audited(&self, inputs: &NativeImageInputs, driver: &Path, executable: &Path) -> Result<NativeLinkAudit, String> {
        self.0.link_audited(inputs, driver, executable)
    }
}
