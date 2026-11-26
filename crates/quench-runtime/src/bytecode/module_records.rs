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

binary_tags!(ModuleReexportKind { Named = 0, Star = 1, Namespace = 2 });

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

binary_tags!(ModuleImportNameKind { Namespace = 0, Named = 1 });

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleRequestPhase {
    Evaluation,
    Defer,
    Source,
}

binary_tags!(ModuleRequestPhase { Evaluation = 0, Defer = 1, Source = 2 });

impl ModuleRequestPhase {
    pub(crate) const fn runtime_value(self) -> f64 {
        self.binary_tag() as f64
    }

    pub(crate) fn from_runtime_value(value: f64) -> Option<Self> {
        [Self::Evaluation, Self::Defer, Self::Source]
            .into_iter()
            .find(|phase| phase.runtime_value() == value)
    }
}
