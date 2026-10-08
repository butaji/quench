//! Guest outcome protocol shared by fixture workers and suite runners.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum NodeOutcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
    GuestExit { code: i32 },
}
