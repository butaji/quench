//! The class layouts the closed world proves, solved once with the facts (`analyze_as`). A class is a
//! constructor: a `new` names its function id. Its instance layout is the ordered named fields every
//! construction stores on `this`, on every path to the constructor's return, and it holds only where no
//! other code reshapes an instance. Its prototype layout is the names the program installs on its
//! `prototype`, and it holds only where no object of unknown class receives a name outside them.
//! Execution reads the layouts (`FunctionFacts::field_slot`); no site re-checks an instance's shape.
//!
//! Order: code that may run while an instance is reachable (a call, a getter, a setter, a store that hands
//! an object out) may store a name on the instance before the constructor's own store, and move its slot.
//! So a path is sealed at the first point that may run guest code (`effects::runs_guest_code`), and an own
//! store of `this` on a sealed path leaves no order. A derived constructor, which binds `this` through
//! `super()`, has no instance layout: the parent's stores are its fields too.

use super::accessors::Accessors;
use super::blocks::Blocks;
use super::census::Census;
use super::flow::FunctionFlow;
use super::quiet::{Proofs, Quiet};
use super::shape::{Change, Owner, Shape};
use crate::bytecode::{ControlEdge, Function, ResidualProgram, Successors};
use crate::dataflow::{Dataflow, solve};
use crate::typed_ir::Classes;
use std::collections::{BTreeMap, BTreeSet};

/// The layouts of the classes the program constructs, each absent where the closed world does not prove it.
#[derive(Default)]
pub(crate) struct ClassLayouts {
    /// Per class, the named fields of its instances in slot order (`AotFacts::class_layout`).
    pub(super) instances: BTreeMap<u32, Box<[u32]>>,
    /// Per class, the names its prototype takes (`AotFacts::prototype_layout`).
    pub(super) prototypes: BTreeMap<u32, Box<[u32]>>,
}

/// The layouts of `classes` (constructor function ids) over the settled `flows` of `program`.
pub(super) fn of_program(program: &ResidualProgram, flows: &[FunctionFlow], census: &Census, classes: &BTreeSet<u32>) -> ClassLayouts {
    let shapes = || flows.iter().flat_map(|flow| flow.shapes.iter());
    let names = prototype_names(flows, census);
    let proofs = Proofs::of(flows);
    let mut layouts = ClassLayouts::default();
    for &class in classes {
        let flow = &flows[class as usize];
        let body = program.with_record(class as usize, |function| body_fields(function, flow, class, &proofs, &census.accessors));
        if let Some(fields) = body.filter(|fields| !shapes().any(|shape| reshapes_instance(shape, class, fields, &census.accessors))) {
            layouts.instances.insert(class, fields.into_boxed_slice());
        }
        let fields = names.get(&class).map_or(&[][..], Vec::as_slice);
        if !shapes().any(|shape| reshapes_prototype(shape, class, fields, census)) {
            layouts.prototypes.insert(class, fields.into());
        }
    }
    layouts
}

/// Per class, the names its prototype takes, in the order the program's code first stores or defines them: the
/// `Field` changes of prototype objects, each resolved to the class whose `prototype` it is (`Census::prototype_owner`).
fn prototype_names(flows: &[FunctionFlow], census: &Census) -> BTreeMap<u32, Vec<u32>> {
    let mut names: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for shape in flows.iter().flat_map(|flow| &flow.shapes) {
        let (Owner::Prototype(base), Change::Field(atom)) = (shape.owner, shape.change) else { continue };
        let Some(class) = census.prototype_owner(base) else { continue };
        let fields = names.entry(class).or_default();
        if !fields.contains(&atom) {
            fields.push(atom);
        }
    }
    names
}

/// Whether `change` names a property `fields` does not list, or a key the analysis cannot name.
fn outside(change: Change, fields: &[u32]) -> bool {
    match change {
        Change::Field(atom) => !fields.contains(&atom),
        Change::Any | Change::Define => true,
    }
}

/// Whether `shape` may add, change or remove a property of an instance of `class` that `fields` does not name.
/// Any such change, by any code, breaks the proof: the constructor's order is the instances' whole shape. A setter call
/// (`claimed_store`) adds no property of its own, so it is no such change.
fn reshapes_instance(shape: &Shape, class: u32, fields: &[u32], accessors: &Accessors) -> bool {
    shape.owner.classes().is_some_and(|classes| classes.may_hold(class)) && outside(shape.change, fields) && !claimed_store(shape, class, accessors)
}

/// Whether `shape` is a store of `class`'s instances that an accessor claims (`Accessors::claims`): a setter call, which
/// creates no field of the instance.
fn claimed_store(shape: &Shape, class: u32, accessors: &Accessors) -> bool {
    match (shape.owner, shape.change) {
        (Owner::This(classes), Change::Field(atom)) => classes.single().is_some_and(|(only, _)| only == class) && accessors.claims(class, atom),
        _ => false,
    }
}

/// Whether `shape` may change the prototype of `class`, whose names are `fields`: its object may be a prototype
/// (an object of unknown class, or a prototype read the census resolves to `class`, or one it does not resolve).
fn reshapes_prototype(shape: &Shape, class: u32, fields: &[u32], census: &Census) -> bool {
    let may_be_prototype = match shape.owner {
        Owner::This(classes) | Owner::Object(classes) => classes == Classes::Many,
        Owner::Prototype(base) => match census.prototype_owner(base) {
            Some(owner) => owner == class,
            None => true,
        },
    };
    may_be_prototype && outside(shape.change, fields)
}

/// The named fields the body of constructor `class` stores on `this`, in order, on every path to its end; None
/// where the paths disagree, where the body changes an object it cannot order (`BodyOrder::step`), or where the
/// body is derived (`super()` binds `this`, so the parent's stores are fields of the instance too).
fn body_fields(function: &Function, flow: &FunctionFlow, class: u32, proofs: &Proofs, accessors: &Accessors) -> Option<Vec<u32>> {
    if (0..function.code.len()).any(|pc| function.instruction_at(pc).op().binds_this()) {
        return None;
    }
    let blocks = Blocks::of(&Successors::of_code(function), function.code.len());
    let mut order = BodyOrder { class, blocks: &blocks, shapes: &flow.shapes[..], guest_runs: &flow.guest_runs[..], quiet: &flow.quiet[..], proofs, accessors };
    let inputs = solve(&mut order);
    let exit = (0..blocks.ranges.len())
        .filter(|&block| blocks.targets[block].is_empty())
        .filter_map(|block| inputs[block].as_ref().map(|input| order.through(block, input.clone())))
        .reduce(Order::merge)?;
    match exit {
        Order::Fields { fields, .. } => Some(fields),
        Order::Conflict => None,
    }
}

/// The named fields a constructor body has stored on `this` so far, in order, and whether a point on the way
/// may have run guest code (`sealed`): no own store may follow one. `Conflict` once two paths store differently,
/// or the body changes an object it cannot order.
#[derive(Clone, PartialEq, Eq)]
enum Order {
    Fields { fields: Vec<u32>, sealed: bool },
    Conflict,
}

impl Order {
    /// The order past a point that may run guest code with the instance reachable.
    fn seal(self) -> Self {
        match self {
            Self::Fields { fields, .. } => Self::Fields { fields, sealed: true },
            Self::Conflict => Self::Conflict,
        }
    }

    /// The order two paths meet in: the same fields, sealed where either path is sealed; otherwise no order.
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Fields { fields, sealed }, Self::Fields { fields: other_fields, sealed: other_sealed }) if fields == other_fields =>
                Self::Fields { fields, sealed: sealed || other_sealed },
            _ => Self::Conflict,
        }
    }
}

/// The dataflow of one constructor body over its basic blocks: the `Order` its shapes build on the way.
struct BodyOrder<'a> {
    class: u32,
    blocks: &'a Blocks,
    shapes: &'a [Shape],
    /// The PCs of the instructions that may run guest code (`FunctionFlow::guest_runs`), in order.
    guest_runs: &'a [u32],
    /// The proofs that a point runs no guest code (`FunctionFlow::quiet`), by PC.
    quiet: &'a [(u32, Quiet)],
    proofs: &'a Proofs,
    /// The accessors, and the this-stores they claim as setter calls (`Accessors::claims`).
    accessors: &'a Accessors,
}

impl BodyOrder<'_> {
    /// The order at the end of `block`, from `order` at its start.
    fn through(&self, block: usize, order: Order) -> Order {
        self.blocks.ranges[block].clone().fold(order, |order, pc| self.at(pc, order))
    }

    /// The order after the instruction at `pc`: sealed first where it may run guest code no proof covers, then changed by
    /// the shapes it makes.
    fn at(&self, pc: usize, order: Order) -> Order {
        let at = pc as u32;
        let from = self.shapes.partition_point(|shape| shape.pc < at);
        let to = self.shapes.partition_point(|shape| shape.pc <= at);
        let order = if self.unproven_run(at, &order) { order.seal() } else { order };
        self.shapes[from..to].iter().fold(order, |order, shape| self.step(order, shape))
    }

    /// Whether the instruction at `at` may run guest code that no proof covers on the path that stored `order`'s fields.
    /// A read of a field the path stored is an own data property, so it runs no getter.
    fn unproven_run(&self, at: u32, order: &Order) -> bool {
        let Order::Fields { fields, .. } = order else { return false };
        if self.guest_runs.binary_search(&at).is_err() {
            return false;
        }
        let from = self.quiet.partition_point(|(pc, _)| *pc < at);
        let to = self.quiet.partition_point(|(pc, _)| *pc <= at);
        !self.quiet[from..to].iter().any(|(_, proof)| match proof {
            Quiet::OwnField(atom) => fields.contains(atom),
            proof => self.proofs.proves(*proof),
        })
    }

    /// The order after `shape`. An object that may be an instance of the class changes in order only through
    /// `this`, by a store of a name, while `this` is exactly that class and no guest code has run since the
    /// constructor began to hold it (`seal`). Any other change to it (a store through another register, a computed
    /// key, a builtin that adds properties) leaves the order unknown. Objects of other classes and prototypes are left alone.
    fn step(&self, order: Order, shape: &Shape) -> Order {
        let Order::Fields { mut fields, sealed } = order else { return Order::Conflict };
        let Some(classes) = shape.owner.classes().filter(|classes| classes.may_hold(self.class)) else {
            return Order::Fields { fields, sealed };
        };
        match (shape.owner, shape.change) {
            // A setter call adds no field, and its guest run sealed the path (`guest_runs`): no own store may follow it.
            _ if claimed_store(shape, self.class, self.accessors) => Order::Fields { fields, sealed: true },
            (Owner::This(_), Change::Field(atom)) if !sealed && classes.single().is_some_and(|(class, _)| class == self.class) => {
                if !fields.contains(&atom) {
                    fields.push(atom);
                }
                Order::Fields { fields, sealed }
            }
            _ => Order::Conflict,
        }
    }
}

impl Dataflow for BodyOrder<'_> {
    type State = Order;

    fn blocks(&self) -> usize {
        self.blocks.ranges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.blocks.targets[block]
    }

    fn entry_state(&self, block: usize) -> Option<Order> {
        (block == 0).then(|| Order::Fields { fields: Vec::new(), sealed: false })
    }

    /// Two paths that disagree on the order make it unknown; a sealed path seals the join.
    fn join(&self, into: &mut Order, incoming: &Order) -> bool {
        let merged = into.clone().merge(incoming.clone());
        let changed = merged != *into;
        *into = merged;
        changed
    }

    fn transfer(&mut self, block: usize, state: &Order) -> Order {
        self.through(block, state.clone())
    }

    /// An exception edge may leave a block after any of its stores, so no order carries along it.
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Order) -> Order {
        match self.blocks.kinds[block][index] {
            ControlEdge::Handler => Order::Conflict,
            ControlEdge::Normal => out.clone(),
        }
    }
}
