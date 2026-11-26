//! The CPU baselines of AOT images and the Cranelift ISA each one names. A baseline is a fixed
//! set of ISA flags on a target triple, never the build host's CPU; host detection stays with
//! the live JIT (`IsaBaseline::Native`, reached only by name).

use cranelift_codegen::isa::{self, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use std::str::FromStr;
use target_lexicon::{Architecture, Triple};

/// A CPU baseline. The x86-64 variants are the psABI levels; the AArch64 ones the Armv8 revisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsaBaseline {
    /// x86-64-v2: SSE4.2, POPCNT, CMPXCHG16B. The x86-64 default.
    X86V2,
    /// x86-64-v3: adds the level's features (`quench_bjit::X86_64_V3_FEATURES`).
    X86V3,
    /// Armv8.0-A. The AArch64 default.
    Armv8,
    /// Armv8.2-A: LSE atomics, FP16 and dot-product.
    Armv82,
    /// The build host's CPU, by host detection. Only when asked for by name.
    Native,
}

/// Each accepted CLI name with its baseline.
const BASELINE_NAMES: &[(&str, IsaBaseline)] = &[
    ("x86-64-v2", IsaBaseline::X86V2),
    ("x86-64-v3", IsaBaseline::X86V3),
    ("armv8", IsaBaseline::Armv8),
    ("armv8.2", IsaBaseline::Armv82),
    ("native", IsaBaseline::Native),
];

const X86_V2_FLAGS: &[&str] = &["has_sse3", "has_ssse3", "has_sse41", "has_sse42", "has_popcnt", "has_cmpxchg16b"];
const ARMV82_FLAGS: &[&str] = &["has_lse", "has_fp16", "has_dotprod"];

impl IsaBaseline {
    /// The baseline a triple's architecture defaults to.
    pub fn default_for(triple: &Triple) -> Self {
        match triple.architecture {
            Architecture::Aarch64 { .. } => Self::Armv8,
            _ => Self::X86V2,
        }
    }

    /// The CLI name of this baseline.
    fn name(self) -> &'static str {
        BASELINE_NAMES.iter().find(|(_, baseline)| *baseline == self).map_or("", |(name, _)| *name)
    }

    /// Whether this baseline is for `architecture` (`Native` is for the host's, checked there).
    fn applies_to(self, architecture: &Architecture) -> bool {
        match self {
            Self::X86V2 | Self::X86V3 => matches!(architecture, Architecture::X86_64),
            Self::Armv8 | Self::Armv82 => matches!(architecture, Architecture::Aarch64 { .. }),
            Self::Native => true,
        }
    }

    /// The Cranelift ISA flags this baseline enables: x86-64-v3's are x86-64-v2's and those of the
    /// level's features Cranelift has a flag for (`quench_bjit::X86_64_V3_FEATURES`).
    fn flags(self) -> Vec<&'static str> {
        match self {
            Self::X86V2 => X86_V2_FLAGS.to_vec(),
            Self::X86V3 => X86_V2_FLAGS.iter().copied().chain(quench_bjit::X86_64_V3_FEATURES.iter().filter_map(|(_, flag)| *flag)).collect(),
            Self::Armv8 | Self::Native => Vec::new(),
            Self::Armv82 => ARMV82_FLAGS.to_vec(),
        }
    }
}

impl Default for IsaBaseline {
    fn default() -> Self {
        Self::default_for(&Triple::host())
    }
}

impl FromStr for IsaBaseline {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        BASELINE_NAMES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, baseline)| *baseline)
            .ok_or_else(|| {
                let accepted: Vec<&str> = BASELINE_NAMES.iter().map(|(known, _)| *known).collect();
                format!("unknown CPU baseline {name}; accepted: {}", accepted.join(", "))
            })
    }
}

/// The machine code target of an AOT image: a CPU baseline on a target triple.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTarget {
    pub baseline: IsaBaseline,
    pub triple: Triple,
}

impl Default for NativeTarget {
    /// The host triple with its architecture's baseline.
    fn default() -> Self {
        let triple = Triple::host();
        Self { baseline: IsaBaseline::default_for(&triple), triple }
    }
}

impl NativeTarget {
    /// The host's CPU on the host triple: what the live JIT compiles for.
    pub(super) fn host() -> Self {
        Self { baseline: IsaBaseline::Native, triple: Triple::host() }
    }

    /// The target the CLI names. An absent triple is the host's; an absent CPU is the triple's default.
    pub fn from_names(cpu: Option<&str>, triple: Option<&str>) -> Result<Self, String> {
        let triple = match triple {
            Some(text) => text.parse::<Triple>().map_err(|error| format!("unknown target {text}: {error}"))?,
            None => Triple::host(),
        };
        let baseline = match cpu {
            Some(name) => name.parse::<IsaBaseline>()?,
            None => IsaBaseline::default_for(&triple),
        };
        let target = Self { baseline, triple };
        target.check()?;
        Ok(target)
    }

    /// Rejects a baseline for another architecture, and `native` for a triple other than the host's.
    fn check(&self) -> Result<(), String> {
        if !self.baseline.applies_to(&self.triple.architecture) {
            return Err(format!("CPU baseline {} does not apply to {}", self.baseline.name(), self.triple));
        }
        if self.baseline == IsaBaseline::Native && self.triple != Triple::host() {
            return Err(format!("CPU baseline native is the host's CPU; {} is not the host", self.triple));
        }
        Ok(())
    }

    /// The Cranelift ISA of this target under the shared `flags` of a publication and level.
    pub(super) fn isa(&self, flags: settings::Flags) -> Result<OwnedTargetIsa, String> {
        self.builder()?.finish(flags).map_err(|error| error.to_string())
    }

    /// The ISA builder for the triple, with the baseline's flags enabled. `Native` is the
    /// host's own detection.
    fn builder(&self) -> Result<isa::Builder, String> {
        if self.baseline == IsaBaseline::Native {
            return host_builder();
        }
        let mut builder = isa::lookup(self.triple.clone()).map_err(|error| format!("{}: {error}", self.triple))?;
        for flag in self.baseline.flags() {
            builder.enable(flag).map_err(|error| format!("{flag}: {error}"))?;
        }
        Ok(builder)
    }
}

fn host_builder() -> Result<isa::Builder, String> {
    cranelift_native::builder().map_err(|error| error.to_string())
}

