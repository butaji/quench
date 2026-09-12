use super::model::{
    GuardKind, GuardSource, PropertyRequirement, PropertyValueKind, QuotedLoop, RegionOp,
    StaticPropertyAccess, visit_ops,
};
use crate::dynbytecode::Register;
use crate::{ObjectCell, ShapeId, Value, dense_array_index};
use std::collections::{BTreeMap, BTreeSet};

pub const PROPERTY_SLOT_VIEW_WORDS: usize = 1;
pub const INLINE_PROPERTY_VIEW_CAPACITY: usize = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertySlotView {
    pub value: *mut Value,
}

const _: () = assert!(
    std::mem::size_of::<PropertySlotView>()
        == PROPERTY_SLOT_VIEW_WORDS * std::mem::size_of::<usize>()
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyAccess {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PropertyShapeGuard {
    identity: *const ObjectCell,
    shape_id: ShapeId,
}

#[derive(Debug)]
struct PropertyShapeChain {
    receiver: PropertyShapeGuard,
    prototypes: Box<[PropertyShapeGuard]>,
}

#[derive(Debug)]
pub struct GuardedProperty {
    pub view: PropertySlotView,
    holder_slot: usize,
    chain: PropertyShapeChain,
    owner: Value,
    value_kind: PropertyValueKind,
}

impl GuardedProperty {
    pub fn receiver_shape_id(&self) -> ShapeId {
        self.chain.receiver.shape_id
    }

    pub fn holder_slot(&self) -> usize {
        self.holder_slot
    }

    pub fn chain_len(&self) -> usize {
        1 + self.chain.prototypes.len()
    }

    pub fn receiver(&self) -> &Value {
        &self.owner
    }

    pub fn into_view(self) -> PropertySlotView {
        self.view
    }

    pub fn is_valid(&self) -> bool {
        let Some(mut current) = self.owner.as_object() else {
            return false;
        };
        let guards = std::iter::once(&self.chain.receiver).chain(self.chain.prototypes.iter());
        for (index, expected) in guards.enumerate() {
            if current.as_ptr() != expected.identity {
                return false;
            }
            let object = current.borrow();
            if object.props.shape_id() != expected.shape_id {
                return false;
            }
            if index + 1 == self.chain_len() {
                let Some((_, value)) = object.props.get_index(self.holder_slot) else {
                    return false;
                };
                return std::ptr::eq(value, self.view.value)
                    && property_value_matches(value, self.value_kind);
            }
            let Some(next) = object.prototype.clone() else {
                return false;
            };
            drop(object);
            current = next;
        }
        false
    }

    pub unsafe fn load_number(&self) -> f64 {
        unsafe {
            (*self.view.value)
                .as_number()
                .expect("guarded property remains numeric")
        }
    }

    pub unsafe fn store_number(&mut self, value: f64) {
        unsafe { std::ptr::write(self.view.value, Value::Number(value)) };
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyGuardFailure {
    NotObject,
    MissingProperty,
    InheritedWrite,
    NotNumber,
    NotDenseArray,
    NotPackedNumber,
    NotTriviallyCopyable,
}

pub fn validate_property(
    owner: Value,
    key: &str,
    access: PropertyAccess,
    value_kind: PropertyValueKind,
) -> Result<GuardedProperty, PropertyGuardFailure> {
    let mut current = owner.as_object().ok_or(PropertyGuardFailure::NotObject)?;
    let receiver_shape_id = current.borrow().props.shape_id();
    let receiver = PropertyShapeGuard {
        identity: current.as_ptr(),
        shape_id: receiver_shape_id,
    };
    let mut prototypes = Vec::new();
    let mut is_receiver = true;
    loop {
        let object = current.borrow();
        if !is_receiver {
            prototypes.push(PropertyShapeGuard {
                identity: current.as_ptr(),
                shape_id: object.props.shape_id(),
            });
        }
        if let Some(slot) = object.props.shape.slot(key) {
            if matches!(access, PropertyAccess::Write) && !prototypes.is_empty() {
                return Err(PropertyGuardFailure::InheritedWrite);
            }
            let value = object
                .props
                .values
                .get(slot)
                .ok_or(PropertyGuardFailure::MissingProperty)?;
            if let Some(failure) = property_value_failure(value, value_kind) {
                return Err(failure);
            }
            let view = PropertySlotView {
                value: std::ptr::from_ref(value).cast_mut(),
            };
            drop(object);
            return Ok(GuardedProperty {
                view,
                holder_slot: slot,
                chain: PropertyShapeChain {
                    receiver,
                    prototypes: prototypes.into_boxed_slice(),
                },
                owner,
                value_kind,
            });
        }
        let Some(next) = object.prototype.clone() else {
            return Err(PropertyGuardFailure::MissingProperty);
        };
        drop(object);
        current = next;
        is_receiver = false;
    }
}

fn property_value_failure(value: &Value, kind: PropertyValueKind) -> Option<PropertyGuardFailure> {
    match kind {
        PropertyValueKind::Number => value
            .as_number()
            .is_none()
            .then_some(PropertyGuardFailure::NotNumber),
        PropertyValueKind::TriviallyCopyable => {
            (!value.is_trivially_copyable()).then_some(PropertyGuardFailure::NotTriviallyCopyable)
        }
        PropertyValueKind::DenseArray => {
            let Some(object) = value.as_object_ref() else {
                return Some(PropertyGuardFailure::NotDenseArray);
            };
            let object = object.borrow();
            let Some(storage) = object.array.as_ref() else {
                return Some(PropertyGuardFailure::NotDenseArray);
            };
            (!storage.is_packed_number()).then_some(PropertyGuardFailure::NotPackedNumber)
        }
    }
}

fn property_value_matches(value: &Value, kind: PropertyValueKind) -> bool {
    property_value_failure(value, kind).is_none()
}

pub fn validate_numeric_property(
    owner: Value,
    key: &str,
    access: PropertyAccess,
) -> Result<GuardedProperty, PropertyGuardFailure> {
    validate_property(owner, key, access, PropertyValueKind::Number)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardPlan {
    pub start: usize,
    pub end: usize,
    pub exit: usize,
    requirements: Box<[(GuardSource, GuardKind)]>,
    internal_number_loads: Box<[usize]>,
    clobbers: Box<[Register]>,
    local_clobbers: Box<[usize]>,
    dense_registers: Box<[Register]>,
    dense_register_bindings: Box<[DenseRegisterPlan]>,
    dense_sites: Box<[DenseSitePlan]>,
    proven_index_sites: Box<[usize]>,
    property_requirements: Box<[PropertyRequirement]>,
    property_sites: Box<[PropertySitePlan]>,
    property_registers: Box<[Register]>,
}

impl GuardPlan {
    pub fn from_region(region: &QuotedLoop) -> Self {
        let requirements: Box<_> = region.requirements().into();
        let (property_requirements, property_sites) =
            derive_property_plan(region.property_requirements());
        let (clobbers, local_clobbers, dense_register_bindings, dense_sites) =
            derive_site_plan(region, &requirements, &property_requirements);
        let dense_registers = dense_register_bindings
            .iter()
            .map(|binding| binding.register)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let property_registers = derive_property_registers(region, &property_requirements);
        Self {
            start: region.start,
            end: region.end,
            exit: region.exit,
            requirements,
            internal_number_loads: region.internal_number_loads().into(),
            clobbers,
            local_clobbers,
            dense_registers,
            dense_register_bindings,
            dense_sites,
            proven_index_sites: region.proven_index_sites().into(),
            property_requirements,
            property_sites,
            property_registers,
        }
    }

    pub fn from_loop(region: &QuotedLoop) -> Self {
        Self::from_region(region)
    }

    pub fn requirements(&self) -> &[(GuardSource, GuardKind)] {
        &self.requirements
    }

    pub fn clobbers(&self) -> &[Register] {
        &self.clobbers
    }

    pub fn local_clobbers(&self) -> &[usize] {
        &self.local_clobbers
    }

    pub fn dense_sites(&self) -> &[DenseSitePlan] {
        &self.dense_sites
    }

    pub fn has_proven_index(&self, pc: usize) -> bool {
        self.proven_index_sites.binary_search(&pc).is_ok()
    }

    pub fn proves(&self, source: &GuardSource, demanded: GuardKind) -> bool {
        self.requirements
            .binary_search_by(|(candidate, _)| candidate.cmp(source))
            .ok()
            .is_some_and(|index| match (self.requirements[index].1, demanded) {
                (GuardKind::ArrayIndex, GuardKind::Number) => true,
                (actual, demanded) => actual == demanded,
            })
    }

    pub fn proves_local_load(&self, pc: usize, slot: usize, demanded: GuardKind) -> bool {
        (demanded == GuardKind::Number && self.internal_number_loads.binary_search(&pc).is_ok())
            || self.proves(&GuardSource::Local(slot), demanded)
    }

    pub fn is_dense_register(&self, register: Register) -> bool {
        self.dense_registers.binary_search(&register).is_ok()
    }

    pub fn dense_register_bindings(&self) -> &[DenseRegisterPlan] {
        &self.dense_register_bindings
    }

    pub fn dense_view_count(&self) -> usize {
        self.requirements
            .iter()
            .filter(|(_, kind)| *kind == GuardKind::DenseArray)
            .count()
            + self
                .property_requirements
                .iter()
                .filter(|requirement| requirement.value_kind == PropertyValueKind::DenseArray)
                .count()
    }

    pub fn property_sites(&self) -> &[PropertySitePlan] {
        &self.property_sites
    }

    pub fn property_requirements(&self) -> &[PropertyRequirement] {
        &self.property_requirements
    }

    pub fn can_reuse_context(&self) -> bool {
        self.property_requirements
            .iter()
            .any(|requirement| requirement.value_kind == PropertyValueKind::DenseArray)
    }

    pub fn is_property_register(&self, register: Register) -> bool {
        self.property_registers.binary_search(&register).is_ok()
    }

    pub fn validate(
        &self,
        mut resolve: impl FnMut(&GuardSource) -> Option<Value>,
    ) -> Result<ValidatedRegionContext, GuardFailure> {
        let mut context = ValidatedRegionContext::default();
        let mut arrays_by_identity = BTreeMap::new();
        for (source, kind) in self.requirements.iter() {
            let value = resolve(source).ok_or_else(|| GuardFailure::Missing(source.clone()))?;
            match kind {
                GuardKind::Number => {
                    value
                        .as_number()
                        .ok_or_else(|| GuardFailure::NotNumber(source.clone()))?;
                }
                GuardKind::ArrayIndex => {
                    value
                        .as_number()
                        .filter(|_| dense_array_index(&value).is_some())
                        .ok_or_else(|| GuardFailure::NotArrayIndex(source.clone()))?;
                }
                GuardKind::DenseArray => validate_array(
                    ArrayInput::Source(source.clone()),
                    source,
                    value,
                    &mut arrays_by_identity,
                    &mut context,
                )?,
            }
        }
        for (property_index, requirement) in self.property_requirements.iter().enumerate() {
            let owner = resolve(&requirement.source)
                .ok_or_else(|| GuardFailure::Missing(requirement.source.clone()))?;
            let access = match requirement.access {
                StaticPropertyAccess::Read => PropertyAccess::Read,
                StaticPropertyAccess::Write => PropertyAccess::Write,
            };
            let property =
                validate_property(owner, &requirement.key, access, requirement.value_kind)
                    .map_err(|failure| GuardFailure::Property {
                        pc: requirement.pc,
                        source: requirement.source.clone(),
                        access: requirement.access,
                        failure,
                    })?;
            if requirement.value_kind == PropertyValueKind::DenseArray {
                let value = unsafe { (&*property.view.value).clone() };
                validate_array(
                    ArrayInput::Property(property_index),
                    &requirement.source,
                    value,
                    &mut arrays_by_identity,
                    &mut context,
                )?;
            }
            context.properties.push(property);
        }
        Ok(context)
    }

    pub fn revalidate(
        &self,
        context: &ValidatedRegionContext,
        mut resolve: impl FnMut(&GuardSource) -> Option<Value>,
    ) -> bool {
        let requirements_hold = self.requirements.iter().all(|(source, kind)| {
            let Some(value) = resolve(source) else {
                return false;
            };
            match kind {
                GuardKind::Number => value.as_number().is_some(),
                GuardKind::ArrayIndex => dense_array_index(&value).is_some(),
                GuardKind::DenseArray => context.matches_source_array(source, &value),
            }
        });
        requirements_hold
            && self
                .property_requirements
                .iter()
                .enumerate()
                .all(|(index, requirement)| {
                    let Some(owner) = resolve(&requirement.source) else {
                        return false;
                    };
                    context.properties.get(index).is_some_and(|property| {
                        property.receiver().0.bits() == owner.0.bits()
                            && property.is_valid()
                            && (requirement.value_kind != PropertyValueKind::DenseArray
                                || context.matches_property_array(index, property))
                    })
                })
    }
}

fn derive_property_plan(
    demands: &[PropertyRequirement],
) -> (Box<[PropertyRequirement]>, Box<[PropertySitePlan]>) {
    let mut indices = BTreeMap::new();
    let mut requirements: Vec<PropertyRequirement> = Vec::new();
    let mut sites = Vec::with_capacity(demands.len());
    for demand in demands {
        let identity = (demand.source.clone(), demand.key.clone());
        let property = *indices.entry(identity).or_insert_with(|| {
            let property = requirements.len();
            requirements.push(demand.clone());
            property
        });
        if matches!(demand.access, StaticPropertyAccess::Write) {
            requirements[property].access = StaticPropertyAccess::Write;
        }
        requirements[property].value_kind = requirements[property]
            .value_kind
            .merge(demand.value_kind)
            .expect("analysis rejects incompatible property result demands");
        sites.push(PropertySitePlan {
            pc: demand.pc,
            property,
        });
    }
    (requirements.into_boxed_slice(), sites.into_boxed_slice())
}

fn derive_property_registers(
    region: &QuotedLoop,
    requirements: &[PropertyRequirement],
) -> Box<[Register]> {
    let sources = requirements
        .iter()
        .map(|requirement| requirement.source.clone())
        .collect::<BTreeSet<_>>();
    let mut definitions = BTreeMap::new();
    let mut registers = BTreeSet::new();
    visit_ops(region.region.node(), &mut |op| {
        update_definition(op, &mut definitions);
        if let Some(destination) = destination(op)
            && definitions
                .get(&destination)
                .is_some_and(|source| sources.contains(source))
        {
            registers.insert(destination);
        }
    });
    registers.into_iter().collect::<Vec<_>>().into_boxed_slice()
}

fn derive_site_plan(
    region: &QuotedLoop,
    requirements: &[(GuardSource, GuardKind)],
    property_requirements: &[PropertyRequirement],
) -> (
    Box<[Register]>,
    Box<[usize]>,
    Box<[DenseRegisterPlan]>,
    Box<[DenseSitePlan]>,
) {
    let array_indices = requirements
        .iter()
        .filter(|(_, kind)| *kind == GuardKind::DenseArray)
        .enumerate()
        .map(|(index, (source, _))| (source.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let dense_source_count = array_indices.len();
    let property_array_indices = property_requirements
        .iter()
        .filter(|requirement| requirement.value_kind == PropertyValueKind::DenseArray)
        .enumerate()
        .map(|(index, requirement)| {
            (
                (requirement.source.clone(), requirement.key.clone()),
                dense_source_count + index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut definitions = BTreeMap::new();
    let mut clobbers = BTreeSet::new();
    let mut local_clobbers = BTreeSet::new();
    let mut observed_locals = BTreeSet::new();
    let mut observed_registers = BTreeSet::new();
    let mut dense_registers = BTreeMap::new();
    let mut dense_sites = Vec::new();
    visit_ops(region.region.node(), &mut |op| {
        match op {
            RegionOp::ReadLocal { slot, .. } => {
                observed_locals.insert(*slot);
            }
            RegionOp::WriteLocal { slot, .. } => {
                if !observed_locals.contains(slot) {
                    local_clobbers.insert(*slot);
                }
                observed_locals.insert(*slot);
            }
            _ => {}
        }
        for register in sources(op) {
            observed_registers.insert(register);
        }
        if let Some(destination) = destination(op) {
            if !observed_registers.contains(&destination) {
                clobbers.insert(destination);
            }
            observed_registers.insert(destination);
        }
        if let Some((pc, object)) = dense_access(op) {
            let array = definitions
                .get(&object)
                .and_then(RegionValueOrigin::dense_array)
                .or_else(|| array_indices.get(&GuardSource::LiveIn(object)).copied())
                .expect("dense source has a guard requirement");
            dense_sites.push(DenseSitePlan { pc, array });
        }
        update_dense_definition(
            op,
            region.internal_local_sources(),
            &array_indices,
            &property_array_indices,
            &mut definitions,
        );
        if let Some(destination) = destination(op)
            && let Some(array) = definitions
                .get(&destination)
                .and_then(RegionValueOrigin::dense_array)
        {
            dense_registers.insert(destination, array);
        }
    });
    (
        clobbers.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        local_clobbers
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        dense_registers
            .into_iter()
            .map(|(register, array)| DenseRegisterPlan { register, array })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        dense_sites.into_boxed_slice(),
    )
}

#[derive(Clone, Debug)]
enum RegionValueOrigin {
    External(GuardSource),
    DenseArray(usize),
}

impl RegionValueOrigin {
    const fn dense_array(&self) -> Option<usize> {
        match self {
            Self::DenseArray(array) => Some(*array),
            Self::External(_) => None,
        }
    }
}

fn update_dense_definition(
    op: &RegionOp,
    internal_local_sources: &BTreeMap<Register, Register>,
    array_indices: &BTreeMap<GuardSource, usize>,
    property_array_indices: &BTreeMap<(GuardSource, String), usize>,
    definitions: &mut BTreeMap<Register, RegionValueOrigin>,
) {
    let destination = destination(op);
    let origin = match op {
        RegionOp::ReadLocal { dst, slot, .. } => internal_local_sources
            .get(dst)
            .and_then(|source| definitions.get(source))
            .cloned()
            .or_else(|| external_origin(GuardSource::Local(*slot), array_indices)),
        RegionOp::ReadCaptured { name, .. } => {
            external_origin(GuardSource::Captured(name.clone()), array_indices)
        }
        RegionOp::Move { src, .. } => definitions.get(src).cloned(),
        RegionOp::ReadStatic { object, key, .. } => definitions
            .get(object)
            .and_then(|origin| match origin {
                RegionValueOrigin::External(source) => property_array_indices
                    .get(&(source.clone(), key.clone()))
                    .copied(),
                RegionValueOrigin::DenseArray(_) => None,
            })
            .map(RegionValueOrigin::DenseArray),
        _ => None,
    };
    if let Some(destination) = destination {
        if let Some(origin) = origin {
            definitions.insert(destination, origin);
        } else {
            definitions.remove(&destination);
        }
    }
}

fn external_origin(
    source: GuardSource,
    array_indices: &BTreeMap<GuardSource, usize>,
) -> Option<RegionValueOrigin> {
    Some(array_indices.get(&source).copied().map_or(
        RegionValueOrigin::External(source),
        RegionValueOrigin::DenseArray,
    ))
}

fn sources(op: &RegionOp) -> Vec<Register> {
    match op {
        RegionOp::WriteLocal { src, .. } | RegionOp::Unary { src, .. } => vec![*src],
        RegionOp::Move { src, .. } => vec![*src],
        RegionOp::Binary { left, right, .. } => vec![*left, *right],
        RegionOp::ReadDense { object, index, .. } => vec![*object, *index],
        RegionOp::WriteDense {
            object, index, src, ..
        } => vec![*object, *index, *src],
        RegionOp::ReadStatic { object, .. } => vec![*object],
        RegionOp::WriteStatic { object, src, .. } => vec![*object, *src],
        RegionOp::JumpIfFalse { test, .. } => vec![*test],
        _ => Vec::new(),
    }
}

fn destination(op: &RegionOp) -> Option<Register> {
    match op {
        RegionOp::NumberLiteral { dst, .. }
        | RegionOp::ReadLocal { dst, .. }
        | RegionOp::ReadCaptured { dst, .. }
        | RegionOp::Move { dst, .. }
        | RegionOp::Unary { dst, .. }
        | RegionOp::Binary { dst, .. }
        | RegionOp::ReadDense { dst, .. }
        | RegionOp::ReadStatic { dst, .. } => Some(*dst),
        _ => None,
    }
}

fn dense_access(op: &RegionOp) -> Option<(usize, Register)> {
    match op {
        RegionOp::ReadDense { pc, object, .. } | RegionOp::WriteDense { pc, object, .. } => {
            Some((*pc, *object))
        }
        _ => None,
    }
}

fn update_definition(op: &RegionOp, definitions: &mut BTreeMap<Register, GuardSource>) {
    match op {
        RegionOp::ReadLocal { dst, slot, .. } => {
            definitions.insert(*dst, GuardSource::Local(*slot));
        }
        RegionOp::ReadCaptured { dst, name, .. } => {
            definitions.insert(*dst, GuardSource::Captured(name.clone()));
        }
        RegionOp::Move { dst, src, .. } => {
            let source = definitions
                .get(src)
                .cloned()
                .unwrap_or(GuardSource::LiveIn(*src));
            definitions.insert(*dst, source);
        }
        _ => {
            if let Some(destination) = destination(op) {
                definitions.remove(&destination);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseSitePlan {
    pub pc: usize,
    pub array: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseRegisterPlan {
    pub register: Register,
    pub array: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertySitePlan {
    pub pc: usize,
    pub property: usize,
}

fn validate_array(
    input: ArrayInput,
    source: &GuardSource,
    owner: Value,
    arrays_by_identity: &mut BTreeMap<u64, usize>,
    context: &mut ValidatedRegionContext,
) -> Result<(), GuardFailure> {
    let receiver_identity = owner.0.bits();
    if let Some(&array) = arrays_by_identity.get(&receiver_identity) {
        context.arrays.push(ArrayBinding { input, array });
        return Ok(());
    }
    let (elements, length, backing_version, prototype_identity) = {
        let object = owner
            .as_object_ref()
            .ok_or_else(|| GuardFailure::NotDenseArray(source.clone()))?
            .borrow();
        let storage = object
            .array
            .as_ref()
            .ok_or_else(|| GuardFailure::NotDenseArray(source.clone()))?;
        if !storage.is_packed_number() {
            return Err(GuardFailure::NotPackedNumber(source.clone()));
        }
        (
            storage.values.as_ptr() as *mut Value,
            storage.len(),
            storage.backing_version,
            object
                .prototype
                .as_ref()
                .map(|prototype| prototype.as_ptr() as usize),
        )
    };
    let array = context.unique_arrays.len();
    context.unique_arrays.push(GuardedArray {
        elements,
        length,
        backing_version,
        prototype_identity,
        owner,
    });
    arrays_by_identity.insert(receiver_identity, array);
    context.arrays.push(ArrayBinding { input, array });
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrayInput {
    Source(GuardSource),
    Property(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrayBinding {
    pub input: ArrayInput,
    pub array: usize,
}

#[derive(Debug)]
pub struct GuardedArray {
    pub elements: *mut Value,
    pub length: usize,
    pub backing_version: u64,
    pub prototype_identity: Option<usize>,
    owner: Value,
}

impl GuardedArray {
    pub fn receiver(&self) -> &Value {
        &self.owner
    }

    fn is_valid(&self) -> bool {
        let Some(object) = self.owner.as_object_ref() else {
            return false;
        };
        let object = object.borrow();
        let Some(storage) = object.array.as_ref() else {
            return false;
        };
        storage.values.as_ptr() == self.elements
            && storage.len() == self.length
            && storage.backing_version == self.backing_version
            && storage.is_packed_number()
            && object
                .prototype
                .as_ref()
                .map(|prototype| prototype.as_ptr() as usize)
                == self.prototype_identity
    }
}

#[derive(Debug, Default)]
pub struct ValidatedRegionContext {
    pub arrays: Vec<ArrayBinding>,
    pub unique_arrays: Vec<GuardedArray>,
    pub properties: ValidatedPropertyViews,
}

impl ValidatedRegionContext {
    fn matches_source_array(&self, source: &GuardSource, value: &Value) -> bool {
        self.arrays.iter().any(|binding| {
            matches!(&binding.input, ArrayInput::Source(candidate) if candidate == source)
                && self.array_matches_value(binding.array, value)
        })
    }

    fn matches_property_array(&self, index: usize, property: &GuardedProperty) -> bool {
        self.arrays.iter().any(|binding| {
            binding.input == ArrayInput::Property(index)
                && unsafe { self.array_matches_value(binding.array, &*property.view.value) }
        })
    }

    fn array_matches_value(&self, array: usize, value: &Value) -> bool {
        self.unique_arrays.get(array).is_some_and(|guarded| {
            guarded.receiver().0.bits() == value.0.bits() && guarded.is_valid()
        })
    }

    pub fn visit_roots(&self, mut visit: impl FnMut(&Value)) {
        self.unique_arrays
            .iter()
            .for_each(|array| visit(array.receiver()));
        self.properties.visit(|property| visit(property.receiver()));
    }
}

#[derive(Debug)]
pub struct ValidatedPropertyViews {
    inline: [Option<GuardedProperty>; INLINE_PROPERTY_VIEW_CAPACITY],
    overflow: Vec<GuardedProperty>,
    len: usize,
}

impl Default for ValidatedPropertyViews {
    fn default() -> Self {
        Self {
            inline: std::array::from_fn(|_| None),
            overflow: Vec::new(),
            len: 0,
        }
    }
}

impl ValidatedPropertyViews {
    fn push(&mut self, property: GuardedProperty) {
        if self.len < INLINE_PROPERTY_VIEW_CAPACITY {
            self.inline[self.len] = Some(property);
        } else {
            self.overflow.push(property);
        }
        self.len += 1;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    fn get(&self, index: usize) -> Option<&GuardedProperty> {
        if index < INLINE_PROPERTY_VIEW_CAPACITY {
            self.inline[index].as_ref()
        } else {
            self.overflow.get(index - INLINE_PROPERTY_VIEW_CAPACITY)
        }
    }

    pub fn visit(&self, mut consume: impl FnMut(&GuardedProperty)) {
        for property in self.inline.iter().flatten().chain(self.overflow.iter()) {
            consume(property);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardFailure {
    Missing(GuardSource),
    NotNumber(GuardSource),
    NotArrayIndex(GuardSource),
    NotDenseArray(GuardSource),
    NotPackedNumber(GuardSource),
    Property {
        pc: usize,
        source: GuardSource,
        access: StaticPropertyAccess,
        failure: PropertyGuardFailure,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric_region::model::{NumericDenseState, Region, RegionNode, RewriteStats};
    use crate::{Object, ObjectHandle, test_object};
    use std::rc::Rc;

    const REGION_START: usize = 4;
    const REGION_EXIT: usize = 9;
    const ARRAY_SLOT: usize = 2;
    const SOURCE_REGISTER: Register = 3;
    const FIRST_PC: usize = 0;
    const SECOND_PC: usize = 1;

    fn plan(requirements: Vec<(GuardSource, GuardKind)>) -> GuardPlan {
        GuardPlan {
            start: REGION_START,
            end: REGION_EXIT,
            exit: REGION_EXIT,
            requirements: requirements.into_boxed_slice(),
            internal_number_loads: Box::new([]),
            clobbers: Box::new([]),
            local_clobbers: Box::new([]),
            dense_registers: Box::new([]),
            dense_register_bindings: Box::new([]),
            dense_sites: Box::new([]),
            proven_index_sites: Box::new([]),
            property_requirements: Box::new([]),
            property_sites: Box::new([]),
            property_registers: Box::new([]),
        }
    }

    fn array(values: Vec<Value>) -> Value {
        Value::Object(test_object(Object::array(None, values)))
    }

    fn object_with_property(key: &str, value: Value) -> ObjectHandle {
        let object = test_object(Object::ordinary(None));
        object.borrow_mut().props.insert(key, value);
        object
    }

    fn dense_property_plan(source: GuardSource, key: &str) -> GuardPlan {
        let mut guard = plan(Vec::new());
        guard.property_requirements = vec![PropertyRequirement {
            pc: FIRST_PC,
            source,
            key: key.into(),
            access: StaticPropertyAccess::Read,
            value_kind: PropertyValueKind::DenseArray,
        }]
        .into_boxed_slice();
        guard
    }

    fn first_write_plan(slot: usize) -> GuardPlan {
        let body: Region<NumericDenseState, NumericDenseState> =
            Region::new(RegionNode::Seq(vec![
                RegionNode::Op(RegionOp::WriteLocal {
                    pc: FIRST_PC,
                    slot,
                    src: SOURCE_REGISTER,
                }),
                RegionNode::Op(RegionOp::Jump {
                    pc: SECOND_PC,
                    target: FIRST_PC,
                }),
            ]));
        GuardPlan::from_loop(&QuotedLoop {
            start: FIRST_PC,
            end: SECOND_PC + 1,
            exit: SECOND_PC + 1,
            region: Region::new(RegionNode::Trace {
                header: FIRST_PC,
                exit: SECOND_PC + 1,
                body: Box::new(body.node),
            }),
            requirements: Vec::new(),
            internal_local_sources: BTreeMap::new(),
            internal_number_loads: Vec::new(),
            proven_index_sites: Vec::new(),
            property_requirements: Vec::new(),
            rewrite_stats: RewriteStats::default(),
        })
    }

    #[test]
    fn packed_numeric_guard_produces_raw_context() {
        let source = GuardSource::Local(ARRAY_SLOT);
        let value = array(vec![Value::Number(3.0), Value::Number(5.0)]);
        let context = plan(vec![(source.clone(), GuardKind::DenseArray)])
            .validate(|candidate| (candidate == &source).then(|| value.clone()))
            .expect("packed array passes");
        assert_eq!(context.unique_arrays.len(), 1);
        assert_eq!(context.unique_arrays[0].length, 2);
        assert!(!context.unique_arrays[0].elements.is_null());
        assert!(context.unique_arrays[0].receiver().is_object());
    }

    #[test]
    fn array_index_guard_rejects_non_index_numbers() {
        let source = GuardSource::Local(ARRAY_SLOT);
        let guard = plan(vec![(source.clone(), GuardKind::ArrayIndex)]);
        for value in [
            Value::Number(-1.0),
            Value::Number(0.5),
            Value::Number(f64::NAN),
            Value::Number(u32::MAX as f64),
        ] {
            assert_eq!(
                guard.validate(|_| Some(value.clone())).unwrap_err(),
                GuardFailure::NotArrayIndex(source.clone())
            );
        }
        assert!(guard.validate(|_| Some(Value::Number(-0.0))).is_ok());
        assert!(
            guard
                .validate(|_| Some(Value::Number(u32::MAX as f64 - 1.0)))
                .is_ok()
        );
    }

    #[test]
    fn holey_array_fails_then_becomes_packed_after_fill() {
        let source = GuardSource::Local(ARRAY_SLOT);
        let value = array(vec![Value::Number(3.0), Value::Undefined]);
        let guard = plan(vec![(source.clone(), GuardKind::DenseArray)]);
        assert_eq!(
            guard.validate(|_| Some(value.clone())).unwrap_err(),
            GuardFailure::NotPackedNumber(source.clone())
        );
        value
            .as_object_ref()
            .unwrap()
            .borrow_mut()
            .array
            .as_mut()
            .unwrap()
            .set(1, Value::Number(5.0));
        assert!(guard.validate(|_| Some(value.clone())).is_ok());
    }

    #[test]
    fn aliasing_sources_share_one_guarded_backing() {
        let first = GuardSource::Local(ARRAY_SLOT);
        let second = GuardSource::Captured("values".into());
        let value = array(vec![Value::Number(1.0)]);
        let context = plan(vec![
            (first.clone(), GuardKind::DenseArray),
            (second.clone(), GuardKind::DenseArray),
        ])
        .validate(|_| Some(value.clone()))
        .expect("aliases are valid");
        assert_eq!(context.unique_arrays.len(), 1);
        assert_eq!(context.arrays[0].array, context.arrays[1].array);
    }

    #[test]
    fn reusable_dense_property_context_rejects_value_and_backing_changes() {
        const PROPERTY_NAME: &str = "values";
        let source = GuardSource::Local(ARRAY_SLOT);
        let first_array = array(vec![Value::Number(1.0)]);
        let receiver = object_with_property(PROPERTY_NAME, first_array.clone());
        let owner = Value::Object(receiver);
        let guard = dense_property_plan(source.clone(), PROPERTY_NAME);
        let resolve = |candidate: &GuardSource| (candidate == &source).then(|| owner.clone());
        let context = guard.validate(resolve).expect("initial context validates");
        assert!(guard.can_reuse_context());
        assert!(guard.revalidate(&context, resolve));

        first_array
            .as_object_ref()
            .unwrap()
            .borrow_mut()
            .array
            .as_mut()
            .unwrap()
            .set(0, Value::Undefined);
        assert!(!guard.revalidate(&context, resolve));

        let replacement = array(vec![Value::Number(2.0)]);
        owner
            .as_object_ref()
            .unwrap()
            .borrow_mut()
            .props
            .insert(PROPERTY_NAME, replacement);
        assert!(!guard.revalidate(&context, resolve));
        assert!(guard.validate(resolve).is_ok());
    }

    #[test]
    fn guard_records_prototype_identity_and_first_local_overwrite() {
        let value = array(vec![Value::Number(1.0)]);
        let prototype = test_object(Object::ordinary(None));
        value.as_object_ref().unwrap().borrow_mut().prototype = Some(prototype);
        let context = plan(vec![(
            GuardSource::Local(ARRAY_SLOT),
            GuardKind::DenseArray,
        )])
        .validate(|_| Some(value.clone()))
        .expect("packed array with prototype passes");
        assert_eq!(
            context.unique_arrays[0].prototype_identity,
            Some(prototype.as_ptr() as usize)
        );
        assert_eq!(first_write_plan(ARRAY_SLOT).local_clobbers(), &[ARRAY_SLOT]);
    }

    #[test]
    fn resize_changes_backing_version_and_holey_state() {
        let value = array(vec![Value::Number(1.0)]);
        let object = value.as_object_ref().unwrap();
        let original_version = object.borrow().array.as_ref().unwrap().backing_version;
        object
            .borrow_mut()
            .array
            .as_mut()
            .unwrap()
            .resize(3, Value::Undefined);
        let object = object.borrow();
        let storage = object.array.as_ref().unwrap();
        assert_ne!(storage.backing_version, original_version);
        assert!(!storage.is_packed_number());
    }

    #[test]
    fn own_numeric_property_view_loads_and_stores_one_word() {
        const PROPERTY_NAME: &str = "coordinate";
        const REPLACEMENT: f64 = 13.0;
        let receiver = object_with_property(PROPERTY_NAME, Value::Number(7.0));
        let mut guarded = validate_numeric_property(
            Value::Object(receiver),
            PROPERTY_NAME,
            PropertyAccess::Write,
        )
        .expect("existing own numeric property validates");

        assert_eq!(guarded.chain_len(), 1);
        assert_eq!(
            guarded.receiver_shape_id(),
            receiver.borrow().props.shape_id()
        );
        assert!(guarded.is_valid());
        assert_eq!(unsafe { guarded.load_number() }, 7.0);
        unsafe { guarded.store_number(REPLACEMENT) };
        assert_eq!(
            receiver
                .borrow()
                .props
                .get(PROPERTY_NAME)
                .and_then(Value::as_number),
            Some(REPLACEMENT)
        );
        assert!(guarded.is_valid());
    }

    #[test]
    fn inherited_view_retains_owner_and_detects_shadowing() {
        const PROPERTY_NAME: &str = "magnitude";
        let holder = object_with_property(PROPERTY_NAME, Value::Number(5.0));
        let receiver = test_object(Object::ordinary(Some(holder)));
        let guarded =
            validate_numeric_property(Value::Object(receiver), PROPERTY_NAME, PropertyAccess::Read)
                .expect("inherited numeric property validates");

        assert_eq!(guarded.chain_len(), 2);
        assert!(guarded.receiver().is_object());
        assert!(guarded.is_valid());
        assert_eq!(unsafe { guarded.load_number() }, 5.0);
        receiver
            .borrow_mut()
            .props
            .insert(PROPERTY_NAME, Value::Number(9.0));
        assert!(!guarded.is_valid());
    }

    #[test]
    fn property_view_keeps_the_receiver_handle_and_rejects_relinking() {
        const PROPERTY_NAME: &str = "weight";
        let holder = object_with_property(PROPERTY_NAME, Value::Number(3.0));
        let receiver = test_object(Object::ordinary(Some(holder)));
        let guarded =
            validate_numeric_property(Value::Object(receiver), PROPERTY_NAME, PropertyAccess::Read)
                .expect("prototype property validates");
        assert_eq!(unsafe { guarded.load_number() }, 3.0);

        let replacement = object_with_property(PROPERTY_NAME, Value::Number(4.0));
        receiver.borrow_mut().prototype = Some(replacement);
        assert!(!guarded.is_valid());
    }

    #[test]
    fn numeric_property_view_rejects_unsafe_categories() {
        const PROPERTY_NAME: &str = "value";
        const EXTERNAL_AND_OBJECT_OWNERS: usize = 2;
        const EXTERNAL_OWNER_ONLY: usize = 1;
        let holder = object_with_property(PROPERTY_NAME, Value::Number(1.0));
        let receiver = test_object(Object::ordinary(Some(holder)));
        assert!(matches!(
            validate_numeric_property(
                Value::Object(receiver),
                PROPERTY_NAME,
                PropertyAccess::Write,
            ),
            Err(PropertyGuardFailure::InheritedWrite)
        ));

        let heap_value = Rc::new("owned".to_owned());
        let object_heap = crate::ObjectHeap::new();
        let receiver = object_heap.allocate(Object::ordinary(None));
        receiver
            .borrow_mut()
            .props
            .insert(PROPERTY_NAME, Value::String(heap_value.clone()));
        assert_eq!(Rc::strong_count(&heap_value), EXTERNAL_AND_OBJECT_OWNERS);
        assert!(matches!(
            validate_numeric_property(Value::Object(receiver), PROPERTY_NAME, PropertyAccess::Read,),
            Err(PropertyGuardFailure::NotNumber)
        ));
        assert!(matches!(
            validate_property(
                Value::Object(receiver),
                PROPERTY_NAME,
                PropertyAccess::Read,
                PropertyValueKind::TriviallyCopyable,
            ),
            Err(PropertyGuardFailure::NotTriviallyCopyable)
        ));
        assert_eq!(
            Rc::strong_count(&heap_value),
            EXTERNAL_AND_OBJECT_OWNERS,
            "rejected property validation does not retain another owner"
        );
        drop(object_heap);
        assert_eq!(Rc::strong_count(&heap_value), EXTERNAL_OWNER_ONLY);
        assert!(matches!(
            validate_numeric_property(Value::Number(1.0), PROPERTY_NAME, PropertyAccess::Read,),
            Err(PropertyGuardFailure::NotObject)
        ));
    }
}
