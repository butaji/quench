//! The records of a module's requests, imports and re-exports, and their binary tags.

#[derive(Clone, Debug)]
pub struct ModuleRequest {
    pub source: String,
    pub phase: ModuleRequestPhase,
    pub module_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ModuleImportBinding {
    pub source: String,
    pub phase: ModuleRequestPhase,
    pub module_type: Option<String>,
    pub imported: ModuleImportName,
    pub local: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleLinkPlan {
    pub locals: Vec<(String, String)>,
    pub reexports: Vec<ModuleReexport>,
    pub hoisted_functions: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub(crate) enum ModuleReexport {
    Named {
        source: String,
        imported: String,
        exported: String,
    },
    Star {
        source: String,
    },
    Namespace {
        source: String,
        exported: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ModuleReexportKind {
    Named,
    Star,
    Namespace,
}

impl ModuleReexportKind {
    const NAMED_BINARY_TAG: u8 = 0;
    const STAR_BINARY_TAG: u8 = 1;
    const NAMESPACE_BINARY_TAG: u8 = 2;

    pub(crate) const fn binary_tag(self) -> u8 {
        match self {
            Self::Named => Self::NAMED_BINARY_TAG,
            Self::Star => Self::STAR_BINARY_TAG,
            Self::Namespace => Self::NAMESPACE_BINARY_TAG,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            Self::NAMED_BINARY_TAG => Some(Self::Named),
            Self::STAR_BINARY_TAG => Some(Self::Star),
            Self::NAMESPACE_BINARY_TAG => Some(Self::Namespace),
            _ => None,
        }
    }
}

impl ModuleReexport {
    pub(crate) const fn kind(&self) -> ModuleReexportKind {
        match self {
            Self::Named { .. } => ModuleReexportKind::Named,
            Self::Star { .. } => ModuleReexportKind::Star,
            Self::Namespace { .. } => ModuleReexportKind::Namespace,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ModuleImportName {
    Namespace,
    Named(String),
}

impl ModuleImportName {
    pub(crate) const fn binary_tag(&self) -> u8 {
        match self {
            Self::Namespace => ModuleImportNameKind::Namespace.binary_tag(),
            Self::Named(_) => ModuleImportNameKind::Named.binary_tag(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ModuleImportNameKind {
    Namespace,
    Named,
}

impl ModuleImportNameKind {
    const fn binary_tag(self) -> u8 {
        match self {
            Self::Namespace => 0,
            Self::Named => 1,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Namespace),
            1 => Some(Self::Named),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleRequestPhase {
    Evaluation,
    Defer,
    Source,
}

impl ModuleRequestPhase {
    const EVALUATION_BINARY_TAG: u8 = 0;
    const DEFER_BINARY_TAG: u8 = 1;
    const SOURCE_BINARY_TAG: u8 = 2;

    pub(crate) const fn binary_tag(self) -> u8 {
        match self {
            Self::Evaluation => Self::EVALUATION_BINARY_TAG,
            Self::Defer => Self::DEFER_BINARY_TAG,
            Self::Source => Self::SOURCE_BINARY_TAG,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            Self::EVALUATION_BINARY_TAG => Some(Self::Evaluation),
            Self::DEFER_BINARY_TAG => Some(Self::Defer),
            Self::SOURCE_BINARY_TAG => Some(Self::Source),
            _ => None,
        }
    }

    pub(crate) const fn runtime_value(self) -> f64 {
        self.binary_tag() as f64
    }

    pub(crate) fn from_runtime_value(value: f64) -> Option<Self> {
        [Self::Evaluation, Self::Defer, Self::Source]
            .into_iter()
            .find(|phase| phase.runtime_value() == value)
    }
}

/// A static module's `throw` of an early error: the value, or an error's name and message.
pub(crate) enum StaticModuleThrow {
    Value(super::Constant),
    Error {
        name: String,
        message: Option<super::Constant>,
    },
}

/// A static module text's plan: its link plan, the requests it makes and whether its top
/// level awaits (`Engine::static_module_analysis`).
#[derive(Clone, Debug, Default)]
pub(crate) struct StaticModulePlan {
    pub(crate) link_plan: Option<ModuleLinkPlan>,
    pub(crate) requests: Vec<ModuleRequest>,
    pub(crate) has_top_level_await: bool,
}


/// `Engine::static_module_analysis`: a static module text's early error, or else its plan
/// (none when the stack does not bound its syntax).
pub(crate) struct StaticModuleAnalysis {
    pub(crate) early_error: Option<String>,
    pub(crate) plan: Option<StaticModulePlan>,
}
