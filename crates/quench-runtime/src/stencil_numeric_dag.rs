//! Bounded Number DAG selection and AArch64 physical composition.
//!
//! Canonical residual instructions remain the semantic authority. This module
//! only proves a terminal, effect-free Number region and selects machine forms
//! for it; unsupported bindings reject before any effect.

use crate::ir::{Instruction, Opcode, Register};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::BinaryOp;
use std::{cell::RefCell, rc::Rc};

pub(crate) const MAX_DAG_INSTRUCTIONS: usize = 32;
const MAX_DAG_VALUES: usize = 24;
const MAX_DAG_INPUTS: usize = 3;
const MAX_DAG_STORES: usize = 12;
const MAX_DAG_ROUTE: usize = 16;
const INVALID_INDEX: u8 = u8::MAX;
const NUMERIC_DAG_REGION_ID: crate::stencil_fact::RegionId =
    crate::stencil_fact::RegionId(0x4e44_4147);

#[derive(Clone, Copy, Debug, PartialEq)]
enum ValueRecipe {
    Empty,
    Input(u8),
    Constant(u64),
    Alias(u8),
    Binary {
        operator: BinaryOp,
        lhs: u8,
        rhs: u8,
    },
    BinaryConstant {
        operator: BinaryOp,
        source: u8,
        bits: u64,
        left: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ValueNode {
    register: Register,
    recipe: ValueRecipe,
}

const EMPTY_NODE: ValueNode = ValueNode {
    register: 0,
    recipe: ValueRecipe::Empty,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalValue {
    slot: u16,
    value: u8,
}

const EMPTY_LOCAL: LocalValue = LocalValue {
    slot: u16::MAX,
    value: INVALID_INDEX,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NumericDagSelection {
    nodes: [ValueNode; MAX_DAG_VALUES],
    inputs: [u16; MAX_DAG_INPUTS],
    stores: [u16; MAX_DAG_STORES],
    route: [Option<&'static str>; MAX_DAG_ROUTE],
    len: u8,
    input_len: u8,
    store_len: u8,
    route_len: u8,
    result: u8,
    span: u8,
}

impl NumericDagSelection {
    pub(crate) const fn span(self) -> usize {
        self.span as usize
    }

    pub(crate) fn route(self) -> impl Iterator<Item = &'static str> {
        self.route
            .into_iter()
            .take(usize::from(self.route_len))
            .flatten()
    }

    fn inputs(self, environment: &crate::environment::Environment) -> Option<[f64; 3]> {
        self.stores_are_private(environment).then_some(())?;
        let mut values = [0.0; 3];
        for (index, slot) in self.inputs[..usize::from(self.input_len)]
            .iter()
            .enumerate()
        {
            values[index] = environment.get_number(*slot)?;
        }
        Some(values)
    }

    fn stores_are_private(self, environment: &crate::environment::Environment) -> bool {
        self.stores[..usize::from(self.store_len)]
            .iter()
            .all(|slot| environment.can_elide_terminal_store(*slot))
    }
}

pub(crate) struct NativeNumericDagPlan {
    selection: NumericDagSelection,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<extern "C" fn(f64, f64, f64) -> f64>,
}

impl NativeNumericDagPlan {
    pub(crate) fn new(
        selection: NumericDagSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.local_fusions.numeric().then_some(())?;
        let image = render_aarch64(selection)?;
        Some(Self {
            selection,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(Rc::clone(&owner)),
            image,
        })
    }

    pub(crate) const fn span(&self) -> usize {
        self.selection.span()
    }

    pub(crate) fn route(&self) -> impl Iterator<Item = &'static str> {
        self.selection.route()
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let [first, second, third] = self.selection.inputs(environment)?;
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| call(first, second, third))
            .ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<extern "C" fn(f64, f64, f64) -> f64>> {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_f64x3_entry(address),
            )
            .ok()
    }
}

pub(crate) fn select_numeric_dag_return(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<NumericDagSelection> {
    let mut builder = DagBuilder::new(start);
    let end = entries.len().min(start.checked_add(MAX_DAG_INSTRUCTIONS)?);
    for pc in start..end {
        let instruction = entries.get(pc)?.instruction;
        if instruction.opcode == Opcode::Return {
            return builder.finish(cfg, pc, instruction.a);
        }
        builder.push(code, instruction)?;
    }
    None
}

struct DagBuilder {
    start: usize,
    nodes: [ValueNode; MAX_DAG_VALUES],
    locals: [LocalValue; MAX_DAG_STORES],
    inputs: [u16; MAX_DAG_INPUTS],
    stores: [u16; MAX_DAG_STORES],
    route: [Option<&'static str>; MAX_DAG_ROUTE],
    len: u8,
    local_len: u8,
    input_len: u8,
    store_len: u8,
    route_len: u8,
    arithmetic: u8,
}

impl DagBuilder {
    const fn new(start: usize) -> Self {
        Self {
            start,
            nodes: [EMPTY_NODE; MAX_DAG_VALUES],
            locals: [EMPTY_LOCAL; MAX_DAG_STORES],
            inputs: [u16::MAX; MAX_DAG_INPUTS],
            stores: [u16::MAX; MAX_DAG_STORES],
            route: [None; MAX_DAG_ROUTE],
            len: 0,
            local_len: 0,
            input_len: 0,
            store_len: 0,
            route_len: 0,
            arithmetic: 0,
        }
    }

    fn push(&mut self, code: CodeView<'_>, instruction: Instruction) -> Option<()> {
        match instruction.opcode {
            Opcode::LoadLocal | Opcode::LoadLocalChecked => self.load_local(instruction),
            Opcode::LoadConst => self.load_constant(code, instruction),
            Opcode::Move if instruction.flags == 0 => self.alias(instruction),
            Opcode::StoreLocal => self.store_local(instruction),
            Opcode::AddConst => self.binary_constant(code, instruction),
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => self.binary(instruction),
            _ => None,
        }
    }

    fn load_local(&mut self, instruction: Instruction) -> Option<()> {
        let recipe = match self.local_value(instruction.b) {
            Some(value) => ValueRecipe::Alias(value),
            None => ValueRecipe::Input(self.input(instruction.b)?),
        };
        self.append(instruction.a, recipe)
    }

    fn load_constant(&mut self, code: CodeView<'_>, instruction: Instruction) -> Option<()> {
        let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
            return None;
        };
        self.append(instruction.a, ValueRecipe::Constant(value.to_bits()))
    }

    fn alias(&mut self, instruction: Instruction) -> Option<()> {
        self.append(
            instruction.a,
            ValueRecipe::Alias(self.current(instruction.b)?),
        )
    }

    fn store_local(&mut self, instruction: Instruction) -> Option<()> {
        let value = self.current(instruction.b)?;
        self.bind_local(instruction.a, value)?;
        self.record_store(instruction.a)
    }

    fn binary_constant(&mut self, code: CodeView<'_>, instruction: Instruction) -> Option<()> {
        let crate::ops::Constant::Number(value) = code.constant(instruction.c)? else {
            return None;
        };
        let recipe = ValueRecipe::BinaryConstant {
            operator: BinaryOp::Add,
            source: self.current(instruction.b)?,
            bits: value.to_bits(),
            left: instruction.add_const_is_left(),
        };
        self.append_arithmetic(instruction.a, recipe, "AddConst")
    }

    fn binary(&mut self, instruction: Instruction) -> Option<()> {
        let operator = crate::stencil_plan::numeric_operation(instruction)?;
        let lhs = self.current(instruction.b)?;
        let rhs = self.current(instruction.c)?;
        let label = binary_label(operator, self.is_constant(lhs) || self.is_constant(rhs));
        self.append_arithmetic(
            instruction.a,
            ValueRecipe::Binary { operator, lhs, rhs },
            label,
        )
    }

    fn append_arithmetic(
        &mut self,
        register: Register,
        recipe: ValueRecipe,
        label: &'static str,
    ) -> Option<()> {
        self.append(register, recipe)?;
        self.arithmetic = self.arithmetic.checked_add(1)?;
        self.push_route(label)
    }

    fn append(&mut self, register: Register, recipe: ValueRecipe) -> Option<()> {
        *self.nodes.get_mut(usize::from(self.len))? = ValueNode { register, recipe };
        self.len = self.len.checked_add(1)?;
        Some(())
    }

    fn current(&self, register: Register) -> Option<u8> {
        self.nodes[..usize::from(self.len)]
            .iter()
            .rposition(|node| node.register == register)
            .and_then(|index| u8::try_from(index).ok())
    }

    fn local_value(&self, slot: u16) -> Option<u8> {
        self.locals[..usize::from(self.local_len)]
            .iter()
            .rfind(|binding| binding.slot == slot)
            .map(|binding| binding.value)
    }

    fn bind_local(&mut self, slot: u16, value: u8) -> Option<()> {
        if let Some(binding) = self.locals[..usize::from(self.local_len)]
            .iter_mut()
            .find(|binding| binding.slot == slot)
        {
            binding.value = value;
            return Some(());
        }
        *self.locals.get_mut(usize::from(self.local_len))? = LocalValue { slot, value };
        self.local_len = self.local_len.checked_add(1)?;
        Some(())
    }

    fn input(&mut self, slot: u16) -> Option<u8> {
        if let Some(index) = self.inputs[..usize::from(self.input_len)]
            .iter()
            .position(|input| *input == slot)
        {
            return u8::try_from(index).ok();
        }
        let index = self.input_len;
        *self.inputs.get_mut(usize::from(index))? = slot;
        self.input_len = self.input_len.checked_add(1)?;
        Some(index)
    }

    fn record_store(&mut self, slot: u16) -> Option<()> {
        if self.stores[..usize::from(self.store_len)].contains(&slot) {
            return Some(());
        }
        *self.stores.get_mut(usize::from(self.store_len))? = slot;
        self.store_len = self.store_len.checked_add(1)?;
        Some(())
    }

    fn push_route(&mut self, label: &'static str) -> Option<()> {
        *self.route.get_mut(usize::from(self.route_len))? = Some(label);
        self.route_len = self.route_len.checked_add(1)?;
        Some(())
    }

    fn is_constant(&self, value: u8) -> bool {
        matches!(
            self.nodes.get(usize::from(value)).map(|node| node.recipe),
            Some(ValueRecipe::Constant(_))
        )
    }

    fn finish(
        mut self,
        cfg: &crate::stencil_cfg::ControlFlowFacts,
        return_pc: usize,
        result: Register,
    ) -> Option<NumericDagSelection> {
        let end = return_pc.checked_add(1)?;
        cfg.region_control(self.start, end)?;
        (self.arithmetic >= 2).then_some(())?;
        (self.store_len > 0).then_some(())?;
        self.push_route("Return")?;
        Some(NumericDagSelection {
            nodes: self.nodes,
            inputs: self.inputs,
            stores: self.stores,
            route: self.route,
            len: self.len,
            input_len: self.input_len,
            store_len: self.store_len,
            route_len: self.route_len,
            result: self.current(result)?,
            span: u8::try_from(end.checked_sub(self.start)?).ok()?,
        })
    }
}

fn binary_label(operator: BinaryOp, constant: bool) -> &'static str {
    match (operator, constant) {
        (BinaryOp::Add, true) => "AddConst",
        (BinaryOp::Subtract, true) => "SubConst",
        (BinaryOp::Multiply, true) => "MulConst",
        (BinaryOp::Divide, true) => "DivConst",
        (BinaryOp::Add, false) => "Add",
        (BinaryOp::Subtract, false) => "Sub",
        (BinaryOp::Multiply, false) => "Mul",
        (BinaryOp::Divide, false) => "Div",
        _ => "Binary",
    }
}

#[cfg(target_arch = "aarch64")]
fn render_aarch64(
    selection: NumericDagSelection,
) -> Option<crate::stencil_region_layout::VerifiedRegionImage> {
    let bytes = aarch64::render(selection)?;
    let opcodes = selection_opcodes(selection);
    let identity = crate::stencil_region_layout::RegionImageIdentity {
        key: crate::stencil_fact::RegionKey::from_opcodes(NUMERIC_DAG_REGION_ID, &opcodes),
        cache_signature: byte_fingerprint(&bytes),
        abi: crate::stencil_select::RegionAbi::ScalarF64x3,
    };
    Some(crate::stencil_region_layout::VerifiedRegionImage::from_composed(identity, bytes))
}

#[cfg(not(target_arch = "aarch64"))]
fn render_aarch64(
    _selection: NumericDagSelection,
) -> Option<crate::stencil_region_layout::VerifiedRegionImage> {
    None
}

fn selection_opcodes(selection: NumericDagSelection) -> Vec<Opcode> {
    selection.nodes[..usize::from(selection.len)]
        .iter()
        .filter_map(|node| match node.recipe {
            ValueRecipe::Binary { operator, .. } | ValueRecipe::BinaryConstant { operator, .. } => {
                opcode_for(operator)
            }
            _ => None,
        })
        .chain(std::iter::once(Opcode::Return))
        .collect()
}

fn opcode_for(operator: BinaryOp) -> Option<Opcode> {
    Opcode::binary_opcode(operator)
}

fn byte_fingerprint(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x1000_0000_01b3;
    bytes.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

#[cfg(target_arch = "aarch64")]
#[path = "stencil_numeric_dag_aarch64.rs"]
mod aarch64;
