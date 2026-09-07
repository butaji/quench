//! Build-time-derived region stencil selection.
//!
//! The runtime selector is intentionally boring: canonicalize facts before
//! this boundary, then perform one key lookup and use the ordinary VM on a
//! miss. Instruction selection and CFG reasoning do not belong here.

use crate::facts::OperationEffect;
pub use crate::stencil_binding::{
    PhysicalBinding, PhysicalBindingValue, PhysicalOperand, PhysicalOperandField, PhysicalOutput,
    PhysicalOutputDestination, PhysicalOutputValue,
};
pub use crate::stencil_cache::{RenderedRegion, RenderedRegionCache, MAX_RENDERED_REGIONS};
use crate::stencil_fact::{FactState, RegionId, RegionKey, Stencil};

/// Physical calling convention declared by a stencil row.  Selection uses
/// the same generated declaration for opcode shape and ABI, so a scalar leaf
/// can never be accidentally invoked with the region-context pointer ABI.
/// Physical boundary properties shared by selection, validation, and the
/// invocation wrappers.  These are consequences of the declared ABI, not a
/// second semantic implementation of the operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AbiContract {
    /// Number of pointer arguments carrying the erased context. Scalar leaves
    /// use typed FP/word arguments and therefore have no context pointer; the
    /// pointed-to `repr(C)` records have their own independently verified
    /// field layout.
    pub context_arg_words: u8,
    /// Whether VM register state is preserved by the physical entry itself.
    /// Raw kernels write only their explicit context/live-out fields.
    pub preserves_vm_registers: bool,
    /// Whether an interior operation may call a Rust helper that can allocate,
    /// throw, call JS, or re-enter. Raw kernels must keep this false.
    pub may_call_helper: bool,
    /// Whether a native backedge contains a runtime interruption checkpoint.
    /// The bounded array loop polls its context flag; larger/unknown spans use
    /// the ordinary interruptible loop.
    pub interruptible_backedge: bool,
    /// Hardware registers the template may clobber. A preserving leaf must
    /// declare an empty mask; bridge/raw entries name their bounded scratch
    /// set so exit materialization can be audited separately.
    pub hardware_clobber_mask: u16,
    /// Integer register destinations written by raw machine templates. This
    /// is separate from the SIMD mask above because AArch64 contexts use x0-x6
    /// for address arithmetic while numeric values live in d0-d2.
    pub hardware_gpr_clobber_mask: u16,
    /// Canonical live-out slots published by the physical entry. Bit zero is
    /// the ordinary result slot; wider masks are reserved for region exits.
    pub live_out_mask: u16,
    /// Whether the caller must expose VM roots before a helper-capable entry.
    pub root_materialization_required: bool,
}

macro_rules! region_abi_catalog {
    ($( $name:ident => {
        context: $region_context:expr,
        context_words: $context_words:expr,
        preserves_vm_registers: $preserves_vm_registers:expr,
        may_call_helper: $may_call_helper:expr,
        interruptible_backedge: $interruptible_backedge:expr,
        hardware_clobber_mask: $hardware_clobber_mask:expr,
        hardware_gpr_clobber_mask: $hardware_gpr_clobber_mask:expr,
        live_out_mask: $live_out_mask:expr,
        root_materialization_required: $root_materialization_required:expr
    }),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum RegionAbi { $( $name ),+ }

        impl RegionAbi {
            pub const fn accepts_region_context(self) -> bool {
                match self { $(Self::$name => $region_context),+ }
            }

            pub const fn contract(self) -> AbiContract {
                match self {
                    $(Self::$name => AbiContract {
                        context_arg_words: $context_words,
                        preserves_vm_registers: $preserves_vm_registers,
                        may_call_helper: $may_call_helper,
                        interruptible_backedge: $interruptible_backedge,
                        hardware_clobber_mask: $hardware_clobber_mask,
                        hardware_gpr_clobber_mask: $hardware_gpr_clobber_mask,
                        live_out_mask: $live_out_mask,
                        root_materialization_required: $root_materialization_required,
                    }),+
                }
            }
        }
    };
}

/// Rank two structurally compatible region candidates without a parallel ABI
/// preference table. The tuple contains only facts carried by the selected
/// physical view: semantic helper boundary, dispatches removed, and code size.
pub(crate) fn admission_rank(record: &RegionRecord) -> (bool, usize, std::cmp::Reverse<usize>) {
    let bytes = select_physical(record.key)
        .map(|view| view.stencil.bytes.len())
        .unwrap_or(usize::MAX);
    (
        !record.template_calls_helper,
        record.operations.len().saturating_sub(1),
        std::cmp::Reverse(bytes),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionRecord {
    /// Stable build-time declaration name used only for diagnostics and
    /// storage attribution; semantic selection remains keyed by `RegionKey`.
    pub name: &'static str,
    pub key: RegionKey,
    pub stencil: Stencil,
    pub operations: &'static [crate::ir::Opcode],
    /// Physical operand relationships generated from the recipe declaration.
    /// These constrain wiring only; opcode semantics remain canonical IR facts.
    pub bindings: &'static [PhysicalBinding],
    /// Exact VM locations reconstructed from the native context at exit.
    pub outputs: &'static [PhysicalOutput],
    pub entry: u16,
    /// All legal external entry offsets, generated from the declaration.
    /// Runtime admission may enter only at one of these boundaries.
    pub external_entries: &'static [u16],
    pub fallthrough: Option<PhysicalFallthrough>,
    pub abi: RegionAbi,
    /// Canonical semantic-boundary fact for the selected template. Runtime
    /// validation still checks physical instructions fail-closed; a branch is
    /// not treated as a helper merely because its encoding resembles one.
    pub template_calls_helper: bool,
    /// Some regions describe IC data/layout but do not yet contain a complete
    /// executable semantic leaf.  Those rows remain selectable for auditing,
    /// but the renderer must use the canonical fallback for them.
    pub executable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalFallthrough {
    pub stencil: &'static Stencil,
    pub target: &'static str,
}

/// The mechanical semantic contract of a generated region row.  The row's
/// operation sequence remains the single source of truth: effects, control
/// shape, and result/live-state requirements are queried from the canonical
/// opcode declarations instead of being copied into a stencil-specific table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionContract {
    pub abi: RegionAbi,
    pub operations: &'static [crate::ir::Opcode],
    pub entry: u16,
    pub external_entries: &'static [u16],
    pub executable: bool,
    pub template_calls_helper: bool,
}

impl RegionContract {
    pub const fn legal_external_entry(self, offset: u16) -> bool {
        if offset != self.entry {
            return false;
        }
        let mut index = 0;
        while index < self.external_entries.len() {
            if self.external_entries[index] == offset {
                return true;
            }
            index += 1;
        }
        false
    }

    pub fn has_effect(self, effect: OperationEffect) -> bool {
        self.operations
            .iter()
            .copied()
            .any(|opcode| opcode.has_effect(effect))
    }

    pub const fn has_control_effect(self) -> bool {
        let mut index = 0;
        while index < self.operations.len() {
            if self.operations[index].has_effect(OperationEffect::Control) {
                return true;
            }
            index += 1;
        }
        false
    }

    pub fn requires_semantic_boundary(self) -> bool {
        self.has_effect(OperationEffect::Allocate)
            || self.has_effect(OperationEffect::MayThrow)
            || self.has_effect(OperationEffect::Observable)
    }

    pub const fn abi_contract(self) -> AbiContract {
        self.abi.contract()
    }

    pub const fn has_single_entry(self) -> bool {
        self.external_entries.len() == 1 && self.external_entries[0] == self.entry
    }

    pub const fn abi_is_well_formed(self) -> bool {
        let abi = self.abi_contract();
        let context_arity_ok = if self.abi.accepts_region_context() {
            abi.context_arg_words == 1
        } else {
            abi.context_arg_words == 0
        };
        context_arity_ok
            && (!abi.preserves_vm_registers
                || (abi.hardware_clobber_mask == 0 && abi.hardware_gpr_clobber_mask == 0))
            && (abi.preserves_vm_registers
                || abi.hardware_clobber_mask != 0
                || abi.hardware_gpr_clobber_mask != 0)
            && (!self.operations.is_empty() || abi.live_out_mask == 0)
            && (!abi.may_call_helper || abi.root_materialization_required)
            && (!abi.interruptible_backedge || self.has_control_effect())
    }
}

impl RegionRecord {
    /// Build the contract from this generated row.  No caller may construct a
    /// second effect/ABI universe: operation effects and control facts are
    /// always read from `ir::Opcode::spec` through this view.
    pub const fn contract(&self) -> RegionContract {
        RegionContract {
            abi: self.abi,
            operations: self.operations,
            entry: self.entry,
            external_entries: self.external_entries,
            executable: self.executable,
            template_calls_helper: self.template_calls_helper,
        }
    }
}

/// One verified physical selection.  Canonical semantic facts stay in the
/// catalog record; the view only couples those facts to the exact bytes that
/// will be rendered, preventing generated/legacy metadata from being mixed.
#[derive(Clone, Copy, Debug)]
pub struct PhysicalStencilView {
    pub key: RegionKey,
    pub record: &'static RegionRecord,
    pub stencil: &'static Stencil,
    pub generated: bool,
    /// Stable identity of the selected physical artifact.
    pub artifact_id: &'static str,
    /// Immutable payload kept separate from executable code bytes.
    pub data: &'static [u8],
    pub compiler: Option<&'static str>,
    /// Relocations validated against the Rust object and carried with the
    /// selected bytes. Targets are physical labels, never semantic op names.
    pub relocations: &'static [PhysicalRelocation],
    /// Identity and physical boundary facts travel with the selected bytes.
    /// Callers must not pair an artifact with independently looked-up record
    /// metadata after this point.
    pub abi: RegionAbi,
    pub entry: u16,
    pub external_entries: &'static [u16],
    pub fallthrough: Option<PhysicalFallthrough>,
    pub executable: bool,
    pub template_calls_helper: bool,
    pub target: Option<&'static str>,
    pub fingerprint: Option<&'static str>,
}

impl PhysicalStencilView {
    /// Rebuild the semantic contract with the selected physical boundary.
    /// Operation effects remain borrowed from the canonical record; physical
    /// ABI/layout facts come only from this view.
    pub const fn contract(&self) -> RegionContract {
        RegionContract {
            abi: self.abi,
            operations: self.record.operations,
            entry: self.entry,
            external_entries: self.external_entries,
            executable: self.executable,
            template_calls_helper: self.template_calls_helper,
        }
    }

    pub fn matches(&self, other: &Self) -> bool {
        std::ptr::eq(self.record, other.record)
            && self.key == other.key
            && self.artifact_id == other.artifact_id
            && self.data == other.data
            && self.compiler == other.compiler
            && self.relocations == other.relocations
            && self.abi == other.abi
            && self.entry == other.entry
            && self.external_entries == other.external_entries
            && self.stencil.bytes == other.stencil.bytes
            && self.stencil.holes == other.stencil.holes
            && self.fallthrough == other.fallthrough
            && self.executable == other.executable
            && self.template_calls_helper == other.template_calls_helper
            && self.target == other.target
            && self.fingerprint == other.fingerprint
    }

    /// Derive the executable-cache identity from this complete physical view.
    /// Hole-free artifacts are independent of disposable site facts; patched
    /// artifacts include those facts plus the generated/legacy artifact ID.
    pub(crate) fn cache_signature<const N: usize>(
        self,
        values: &crate::stencil_fact::PatchValues<'_, N>,
    ) -> u64 {
        let patch = if self.stencil.holes.is_empty() {
            0
        } else {
            values.signature()
        };
        physical_identity_hash(self, patch)
    }
}

fn physical_identity_hash(view: PhysicalStencilView, patch: u64) -> u64 {
    let mut hash = patch.wrapping_add(0xcbf2_9ce4_8422_2325);
    let identity = view
        .artifact_id
        .as_bytes()
        .iter()
        .chain(view.fingerprint.unwrap_or_default().as_bytes());
    for byte in identity {
        hash = hash
            .wrapping_mul(0x1000_0000_01b3)
            .wrapping_add(u64::from(*byte));
    }
    hash.max(1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRelocation {
    pub offset: u16,
    pub kind: crate::stencil_fact::HoleKind,
    pub target: &'static str,
    pub addend: i64,
}

include!(concat!(env!("OUT_DIR"), "/stencil_catalog.rs"));
include!(concat!(env!("OUT_DIR"), "/stencil_artifacts.rs"));

include!("stencil_physical_select.rs");

#[cfg(test)]
#[path = "stencil_select_contract_tests.rs"]
mod generated_region_admission_tests;

include!("stencil_select_optimizer.rs");

#[cfg(test)]
#[path = "stencil_select_artifact_tests.rs"]
mod artifact_tests;
#[cfg(test)]
#[path = "stencil_select_tests.rs"]
mod tests;
