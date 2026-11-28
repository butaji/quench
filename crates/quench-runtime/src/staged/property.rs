//! The named-property family as staged rows: `GetField` in every form (own slot, prototype
//! slot, prototype constant, polymorphic arms, nested chains, a VM probe behind a fast plan),
//! `SetField` and `StoreField` (own slots in place by shape arms, a loop's projected slot, the
//! VM's own-field, bound-slot and adding stores), `Delete` and `in`. A site's proved plan is
//! static data (`FieldPlan`, `StorePlan`); each row runs it once over a stage, so the interpreter
//! (`Concrete`) and every specializer (`Emit`) read one definition of each read and store.
//! Whatever the plan cannot prove is an explicit `PropertyFrame` call-out to the VM's helper,
//! never a second copy of the lookup. What a tier proved of an object before the row runs (its
//! record, its shape, its layout) is read back through the frame, so a guard the frame holds
//! the proof of is not emitted twice.

#[path = "property_slots.rs"]
mod slots;

use slots::{formed_slot, kept_slot, word_formed_slot};

pub use slots::guard_receiver_layout;

use super::ic::{self, IcOp, Reached};
use super::object::{object_record_of, Form, Layout};
use super::{value, Frame, NumericFrame, NumericStage, Operation, Stage, Step};
use crate::bytecode::{FieldBase, FieldLookup, Register};
use crate::typed_ir::Type;

#[path = "property/plan.rs"]
mod plan;
#[path = "property/store_plan.rs"]
mod store_plan;
#[path = "property/frame.rs"]
mod frame;
#[path = "property/read.rs"]
mod read;
#[path = "property/get.rs"]
mod get;
pub use plan::*;
pub use store_plan::*;
pub use frame::*;
pub use read::*;
pub use get::*;
