//! Outcome vocabulary shared by the isolated Node fixture workers.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum NodeOutcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
    GuestExit { code: i32 },
}
