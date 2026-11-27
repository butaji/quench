//! The accessors the program defines (`get x()`, `set x(v)`) on a class, what may shadow a named property on an object, and the
//! `extends` links a class's prototype chain makes.
//! A read or store of an accessor's name on an object runs the accessor found first on the object's prototype chain. The closed
//! world proves that access a call of that accessor's function (`Target::Accessor`, `types::targets`) only where no store may
//! write the name on a class the chain passes, and no new code may move it there. The same chain and shadows bound
//! prototype-chain folding (`Types::holder`): a field a store may write on a class below a holder is not the holder's to fold to.
//! A `this.x = v` of a name an accessor on the class's chain claims is that setter call, not a field the instance gets
//! (`Accessors::claims`).

use super::super::call_plan::Arm;
use super::census::Census;
use super::effects::{Ctx, Events, base_fact, decoded, this_fact};
use super::flow::{Call, FunctionFlow, Target};
use super::shape::{Change, Owner, Shape};
use super::value::{Accessor, Key, Origin, PrototypeBase, Word};
use crate::bytecode::{PropertyDefinitionMode, WideInstruction};
use crate::staged::Operation;
use crate::typed_ir::{Classes, FactKey};
use std::collections::{BTreeMap, BTreeSet};

/// The stores that may write a named property, as the census sees them: the names some store writes on an object of no
/// one class (`names`), the (class, name) pairs a store writes on a class's objects or its prototype (`classes`), and
/// whether a store the analysis cannot name may change any property of any object (`wild`). An accessor definition is no
/// store of its name, and neither is a this-store an accessor claims (`Accessors::claims`).
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Shadows {
    names: BTreeSet<u32>,
    classes: BTreeSet<(u32, u32)>,
    wild: bool,
}

impl Shadows {
    /// Records what `shape` may write, on the objects of its owner's classes or of a prototype's class, and any change to a
    /// key the analysis cannot name.
    fn shape(&mut self, shape: &Shape, census: &Census) {
        match (shape.change, shape.owner) {
            (Change::Any | Change::Define, _) => self.wild = true,
            (Change::Field(atom), Owner::Prototype(base)) => match census.prototype_owner(base) {
                Some(class) => { self.classes.insert((class, atom)); }
                None => { self.names.insert(atom); }
            },
            (Change::Field(atom), Owner::This(classes) | Owner::Object(classes)) => self.field(classes, atom),
        }
    }

    /// Records a store of `atom` on the objects of `classes`: a name on no one class, or on each class's objects.
    fn field(&mut self, classes: Classes, atom: u32) {
        match classes {
            Classes::Many => { self.names.insert(atom); }
            _ => self.classes.extend(classes.ids().iter().map(|&class| (class, atom))),
        }
    }

    /// Whether a store may write named property `atom` on an object of class `class`, or of no one class.
    pub(super) fn writes(&self, class: u32, atom: u32) -> bool {
        self.wild || self.names.contains(&atom) || self.classes.contains(&(class, atom))
    }

    /// Whether a store may write named property `atom` on an object of no one class, or one the analysis cannot name.
    pub(super) fn writes_any(&self, atom: u32) -> bool {
        self.wild || self.names.contains(&atom)
    }

    /// Whether a store may write named property `atom` on the objects of class `class` (`writes`).
    pub(super) fn writes_class(&self, class: u32, atom: u32) -> bool {
        self.classes.contains(&(class, atom))
    }
}

/// The getter and setter one class defines for a name, each where the solve names its function.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Pair {
    getter: Option<u32>,
    setter: Option<u32>,
}

impl Pair {
    /// The pair with `accessor` defined in it: None where the definition names no function, or the other definition of its
    /// kind is already there (the later one wins at run time, so the solve keeps neither).
    fn with(self, accessor: Accessor) -> Option<Self> {
        let function = accessor.function?;
        let mut pair = self;
        let field = if accessor.setter { &mut pair.setter } else { &mut pair.getter };
        if field.is_some() { return None; }
        *field = Some(function);
        Some(pair)
    }
}

/// The accessors the program defines, by class and name, the `extends` links of the classes, the this-stores an accessor
/// claims, and what may shadow the accessors.
#[derive(Default)]
pub(super) struct Accessors {
    /// Per (class, instance, name), the accessors it defines; None where a definition names no function, or names one a
    /// second definition of the same kind also defines.
    defined: BTreeMap<(u32, bool, u32), Option<Pair>>,
    /// Every function an accessor definition names, on any object: each escapes (`escapes`), as code the solve does not
    /// see (a builtin, a spread, a `JSON` walk) may run an accessor with any receiver.
    functions: BTreeSet<u32>,
    /// Every name an accessor definition defines, on any object (`names_of`).
    names: BTreeSet<u32>,
    /// The parent each class's prototype chain links it to (`parents_of`).
    parents: BTreeMap<u32, u32>,
    /// The (class, name) pairs whose this-stores are setter calls (`claims_store`): the store creates no field of the instance.
    claimed: BTreeSet<(u32, u32)>,
    pub(super) shadows: Shadows,
}

impl Accessors {
    /// The accessors of `flows`, the chains they link, and the stores that may shadow them, over the census of the same flows.
    pub(super) fn of(flows: &[FunctionFlow], census: &Census) -> Self {
        let mut accessors = Self {
            defined: BTreeMap::new(),
            functions: BTreeSet::new(),
            names: names_of(flows),
            parents: parents_of(flows),
            claimed: BTreeSet::new(),
            shadows: Shadows::default(),
        };
        for accessor in flows.iter().flat_map(|flow| &flow.accessors) {
            accessors.functions.extend(accessor.function);
            let Some((class, instance)) = accessor.site else { continue };
            let slot = accessors.defined.entry((class, instance, accessor.atom)).or_insert(Some(Pair::default()));
            let current = *slot;
            *slot = current.and_then(|pair| pair.with(*accessor));
        }
        accessors.shadows = accessors.shadows_of(flows, census);
        accessors
    }

    /// The stores that may shadow the accessors (`shadows`): the stores of `flows` but the accessor definitions, and the
    /// this-stores an accessor claims (`claims_store`), which create no field. A this-store no accessor claims writes the
    /// objects of its classes, as any store does.
    fn shadows_of(&mut self, flows: &[FunctionFlow], census: &Census) -> Shadows {
        let definitions: BTreeSet<(usize, u32)> = flows.iter().enumerate()
            .flat_map(|(id, flow)| flow.accessors.iter().map(move |accessor| (id, accessor.pc)))
            .collect();
        let mut base = Shadows::default();
        let mut own: Vec<(Classes, u32)> = Vec::new();
        for (id, flow) in flows.iter().enumerate() {
            base.wild |= flow.member_wild;
            base.names.extend(flow.writes.iter().filter_map(|(key, _)| match key {
                Key::MemberAny(atom) => Some(*atom),
                _ => None,
            }));
            for shape in flow.shapes.iter().filter(|shape| !definitions.contains(&(id, shape.pc))) {
                match (shape.owner, shape.change) {
                    (Owner::This(classes), Change::Field(atom)) => own.push((classes, atom)),
                    _ => base.shape(shape, census),
                }
            }
        }
        let mut shadows = base.clone();
        for (classes, atom) in own {
            if classes == Classes::Many {
                shadows.field(classes, atom);
                continue;
            }
            for &class in classes.ids() {
                if self.claims_store(census, &base, class, atom) {
                    self.claimed.insert((class, atom));
                } else {
                    shadows.classes.insert((class, atom));
                }
            }
        }
        shadows
    }

    /// Whether a this-store of `atom` on an instance of `class` is a setter call: the first class up the chain that holds
    /// `atom` on its prototype defines an accessor for it, and no class before it takes a data property of `atom` (`base`,
    /// the stores but the this-stores) or gains one from new code (`World::live`).
    fn claims_store(&self, census: &Census, base: &Shadows, class: u32, atom: u32) -> bool {
        if census.live(FactKey::AnyField(atom)) { return false; }
        let mut current = class;
        for _ in 0..=self.parents.len() {
            if census.live(FactKey::Field { class: current, instance: true, atom }) || base.writes(current, atom) { return false; }
            if self.defined.contains_key(&(current, true, atom)) { return true; }
            match self.parents.get(&current) {
                Some(&parent) => current = parent,
                None => return false,
            }
        }
        false
    }

    /// Whether a this-store of `atom` on the instances of `class` is a setter call (`claims_store`), so it creates no field.
    pub(super) fn claims(&self, class: u32, atom: u32) -> bool {
        self.claimed.contains(&(class, atom))
    }

    /// The `extends` links of the classes (`parents_of`): the chain `accessor_function` walks, which `Types::holder` walks too.
    pub(super) fn parents(&self) -> &BTreeMap<u32, u32> {
        &self.parents
    }

    /// Every function an accessor definition names (`functions`).
    pub(super) fn functions(&self) -> impl Iterator<Item = u32> + '_ {
        self.functions.iter().copied()
    }

    /// Whether the program defines an accessor of name `atom` on some object: an access of the name the census proves no
    /// accessor of may run the program's accessor (`escapes`).
    pub(super) fn defines(&self, atom: u32) -> bool {
        self.names.contains(&atom)
    }
}

/// The names the accessor definitions of `flows` define, on any object (`Accessors::names`): a store of one of them may run
/// its setter, so the solve keeps such a store a guest run (`effects::runs_guest_code`).
pub(super) fn names_of(flows: &[FunctionFlow]) -> BTreeSet<u32> {
    flows.iter().flat_map(|flow| &flow.accessors).map(|accessor| accessor.atom).collect()
}

/// The parent each class's prototype chain links it to (`FunctionFlow::parents`): where every link the flows make names the
/// same parent. A class whose links disagree, or that some link names no parent for, keeps none, so the chain stops there.
fn parents_of(flows: &[FunctionFlow]) -> BTreeMap<u32, u32> {
    let mut parents: BTreeMap<u32, Option<u32>> = BTreeMap::new();
    for &(child, parent) in flows.iter().flat_map(|flow| &flow.parents) {
        let joined = parents.entry(child).or_insert(Some(parent));
        if *joined != Some(parent) { *joined = None; }
    }
    parents.into_iter().filter_map(|(child, parent)| Some((child, parent?))).collect()
}

/// Whether a property definition of `mode` defines an accessor: `Some(true)` for a setter, `Some(false)` for a getter, and
/// `None` for a method or a data property.
pub(super) fn accessor_mode(mode: PropertyDefinitionMode) -> Option<bool> {
    match mode {
        PropertyDefinitionMode::Getter | PropertyDefinitionMode::EnumerableGetter => Some(false),
        PropertyDefinitionMode::Setter | PropertyDefinitionMode::EnumerableSetter => Some(true),
        PropertyDefinitionMode::Method | PropertyDefinitionMode::ReadonlyMethod => None,
    }
}

/// Records the accessor a `get` or `set` definition makes at `pc` on `target` under `key`. On a proven class object it is that
/// class's accessor (`Accessor`), stored under its key with the cell no analysis names, so a read of the name there is any
/// value wherever the class does not shadow it. Anywhere else it is a store under the name's `MemberAny` key. The name's `Member`
/// key takes the cell too, so a read off an object of no one class is any value. A computed key may name any property.
pub(super) fn accessor_definition(pc: usize, setter: bool, target: Word, key: Word, stored: Word, events: &mut Events) {
    let Some(atom) = key.atom_key() else {
        events.wild_member = true;
        return;
    };
    events.writes.push((Key::Member(atom), Word::UNKNOWN));
    let site = match target.class_site() {
        Some((class, instance, false)) => Some((class, instance)),
        _ => None,
    };
    events.accessors.push(Accessor { site, atom, setter, function: stored.cell.ty.body_index(), pc: pc as u32 });
    match site {
        Some((class, instance)) => events.writes.push((Key::MemberOn { class, instance, atom }, Word::UNKNOWN)),
        None => events.writes.push((Key::MemberAny(atom), Word::UNKNOWN)),
    }
}

/// The access a named field read or store makes of its receiver, when the receiver may run an accessor of the name
/// (`Target::Accessor`): a getter for a read of a plain field (the operations `value_of` reads as one), and a setter for a
/// store, with the stored value as its argument. `types::targets` names the accessors the census proves; an access none
/// proves is no call.
pub(super) fn accessor_call(ctx: &Ctx<'_>, pc: u32, instruction: WideInstruction, state: &[Word]) -> Option<Call> {
    if matches!(instruction.op().class(), crate::bytecode::Class::GetField) {
        let plain = instruction.result_flags() & !crate::bytecode::SET_THIS_REGISTER == 0 || instruction.returns_from_frame();
        let Some(crate::bytecode::FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup() else { return None };
        if !plain { return None; }
        let receiver = base_fact(ctx, base, state)?;
        return Some(Call { pc, callee: Word::UNKNOWN, target: Target::Accessor { atom, setter: false }, construct: false, receiver, plain: false, arguments: Vec::new() });
    }
    match decoded(instruction)? {
        Operation::SetField { base, atom, value, .. } => {
            let receiver = match base {
                Some(base) => state.get(usize::from(base)).copied()?,
                None if !ctx.is_root => this_fact(ctx),
                None => return None,
            };
            Some(Call { pc, callee: Word::UNKNOWN, target: Target::Accessor { atom, setter: true }, construct: false, receiver, plain: false, arguments: vec![state[usize::from(value)]] })
        }
        _ => None,
    }
}

/// The `extends` link a call makes: `class::set_prototype` links a class's prototype to its heritage's with the intrinsic
/// `Object.setPrototypeOf` (`INTRINSIC_SET_PROTOTYPE_OF_BINDING`), so the constructors the two prototypes are read off are the
/// class and its parent. Any other call, or one of the intrinsic on prototypes no constructor names, links nothing.
pub(super) fn heritage_link(ctx: &Ctx<'_>, call: &Call) -> Option<(u32, u32)> {
    let Some(Origin::Binding(Key::Global(name))) = call.callee.origin else { return None };
    if ctx.scope.set_prototype != Some(name) { return None; }
    let [child, parent] = call.arguments.as_slice() else { return None };
    match (child.origin?, parent.origin?) {
        (Origin::Prototype(PrototypeBase::Closure(class)), Origin::Prototype(PrototypeBase::Closure(base))) => Some((class, base)),
        _ => None,
    }
}

/// The function of the accessor `atom` an access on an object of `class` runs: its getter, or its setter where `setter`.
/// The access walks the object's chain from `class` (an instance's prototype chain, `Accessors::parents`; a class object has
/// none) and takes the first class that holds `atom` as an accessor. It is proven only where no class on the way may take a
/// data property of `atom` (`Shadows::writes`) nor gain one from new code (`World::live`), and the accessor holds a function
/// of that kind; None otherwise, and then the access is no call of the census's.
fn accessor_function(census: &Census, class: u32, instance: bool, atom: u32, setter: bool) -> Option<u32> {
    let accessors = &census.accessors;
    let mut current = class;
    for _ in 0..=accessors.parents.len() {
        if census.live(FactKey::Field { class: current, instance, atom }) || accessors.shadows.writes(current, atom) { return None; }
        if let Some(pair) = accessors.defined.get(&(current, instance, atom)) {
            let pair = (*pair)?;
            return if setter { pair.setter } else { pair.getter };
        }
        match accessors.parents.get(&current) {
            Some(&parent) if instance => current = parent,
            _ => return None,
        }
    }
    None
}

/// The accessor a field access `Target::Accessor` runs, as the arms of its call plan (`call_plan::Arm`): the function each class
/// of the receiver's closed union runs (`accessor_function`), or the one function of a receiver of one class. Empty where any
/// class has none, and then the access is no call.
pub(super) fn accessor_arms(call: &Call, atom: u32, setter: bool, census: &Census) -> Vec<Arm> {
    if census.live(FactKey::AnyField(atom)) { return Vec::new(); }
    let sites: Vec<(Option<u32>, u32, bool)> = match call.receiver.class_site() {
        Some((class, instance, _)) => vec![(None, class, instance)],
        None => call.receiver.cell.classes.ids().iter().map(|&class| (Some(class), class, true)).collect(),
    };
    let arms: Option<Vec<Arm>> = sites.into_iter()
        .map(|(arm_class, class, instance)| accessor_function(census, class, instance, atom, setter).map(|function| Arm { class: arm_class, function }))
        .collect();
    arms.filter(|arms| !arms.is_empty()).unwrap_or_default()
}

/// The accessor one field access runs (`accessor_arms`), with whether it is a setter (its one argument, the stored value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessorArms {
    pub(crate) setter: bool,
    pub(crate) arms: Box<[Arm]>,
}

/// The class a named field read off an object of `site` takes its field from (`Types::holder`): the object's own class, or,
/// for an instance, the holder its prototype chain folds to. Only the solve's types fold, so the first pass reads the class.
pub(super) fn site_holder(ctx: &Ctx<'_>, site: (u32, bool, bool), atom: u32) -> (u32, bool, bool) {
    let (class, instance, guess) = site;
    match ctx.types {
        Some(types) if instance => (types.holder(class, atom), instance, guess),
        _ => site,
    }
}
