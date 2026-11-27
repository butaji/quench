//! The class layouts the closed world proves, solved once with the facts (`analyze_as`). A class is a
//! constructor: a `new` names its function id. Its instance layout is the ordered named fields every
//! construction stores on `this`, on every path to the constructor's return, and it holds only where no
//! other code reshapes an instance. Its prototype layout is the names the program installs on its
//! `prototype`, and it holds only where no object of unknown class receives a name outside them.
//! Execution reads the layouts (`FunctionFacts::field_slot`); no site re-checks an instance's shape.

use super::blocks::Blocks;
use super::census::Census;
use super::flow::FunctionFlow;
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
    let mut layouts = ClassLayouts::default();
    for &class in classes {
        let body = program.with_record(class as usize, |function| body_fields(function, &flows[class as usize].shapes, class));
        if let Some(fields) = body.filter(|fields| !shapes().any(|shape| reshapes_instance(shape, class, fields))) {
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
        Change::Any => true,
        Change::Escape => false,
    }
}

/// Whether `shape` may add, change or remove a property of an instance of `class` that `fields` does not name.
/// Any such change, by any code, breaks the proof: the constructor's order is the instances' whole shape.
fn reshapes_instance(shape: &Shape, class: u32, fields: &[u32]) -> bool {
    shape.owner.classes().is_some_and(|classes| classes.may_hold(class)) && outside(shape.change, fields)
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
/// where the paths disagree, or where the body changes an object it cannot order (`BodyOrder::step`).
fn body_fields(function: &Function, shapes: &[Shape], class: u32) -> Option<Vec<u32>> {
    let blocks = Blocks::of(&Successors::of_code(function), function.code.len());
    let mut order = BodyOrder { class, blocks: &blocks, shapes };
    let inputs = solve(&mut order);
    let exit = (0..blocks.ranges.len())
        .filter(|&block| blocks.targets[block].is_empty())
        .filter_map(|block| inputs[block].as_ref().map(|input| order.through(block, input.clone())))
        .reduce(|a, b| if a == b { a } else { Order::Conflict });
    match exit? {
        Order::Fields(fields) => Some(fields),
        Order::Conflict => None,
    }
}

/// The named fields a constructor body has stored on `this` so far, in order. `Conflict` once two paths store
/// differently, or the body changes an object it cannot order.
#[derive(Clone, PartialEq, Eq)]
enum Order {
    Fields(Vec<u32>),
    Conflict,
}

/// The dataflow of one constructor body over its basic blocks: the `Order` its shapes build on the way.
struct BodyOrder<'a> {
    class: u32,
    blocks: &'a Blocks,
    shapes: &'a [Shape],
}

impl BodyOrder<'_> {
    /// The order at the end of `block`, from `order` at its start.
    fn through(&self, block: usize, order: Order) -> Order {
        let range = &self.blocks.ranges[block];
        let from = self.shapes.partition_point(|shape| (shape.pc as usize) < range.start);
        let to = self.shapes.partition_point(|shape| (shape.pc as usize) < range.end);
        self.shapes[from..to].iter().fold(order, |order, shape| self.step(order, shape))
    }

    /// The order after `shape`. An object that may be an instance of the class changes in order only through
    /// `this`, by a store of a name, while `this` is exactly that class: any other change to it (a store through
    /// another register, a computed key, a call that may run code with it) leaves the order unknown. Objects of
    /// other classes and prototypes are left alone.
    fn step(&self, order: Order, shape: &Shape) -> Order {
        let Order::Fields(mut fields) = order else { return Order::Conflict };
        let Some(classes) = shape.owner.classes().filter(|classes| classes.may_hold(self.class)) else {
            return Order::Fields(fields);
        };
        match (shape.owner, shape.change) {
            (Owner::This(_), Change::Field(atom)) if classes.single().is_some_and(|(class, _)| class == self.class) => {
                if !fields.contains(&atom) {
                    fields.push(atom);
                }
                Order::Fields(fields)
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
        (block == 0).then(|| Order::Fields(Vec::new()))
    }

    /// Two paths that disagree on the order, or one that is unknown, make the order unknown.
    fn join(&self, into: &mut Order, incoming: &Order) -> bool {
        if *into == *incoming || *into == Order::Conflict {
            return false;
        }
        *into = Order::Conflict;
        true
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
