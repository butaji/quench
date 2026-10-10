#![allow(dead_code)]

use std::mem::{ManuallyDrop, size_of};
use std::rc::Rc;

#[derive(Clone, Copy)]
#[repr(C)]
struct ObjectData {
    property_storage: u64,
    inline_values: [u64; 2],
}

#[repr(C)]
struct ArrayData {
    property_storage: u64,
    elements: ManuallyDrop<Rc<Vec<u64>>>,
    inline_property: u64,
}

enum ColdCell {
    String(Vec<u16>),
    Function([u64; 4]),
    Other([u64; 12]),
}

#[repr(u8)]
enum Cell {
    Object(ObjectData),
    Array(ArrayData),
    Cold(Box<ColdCell>),
}

struct Slot {
    cell: Option<Cell>,
}

fn main() {
    println!(
        "object_data={} array_data={} cold_enum={} cell={} option_cell={} slot={} rc_vec={}",
        size_of::<ObjectData>(),
        size_of::<ArrayData>(),
        size_of::<ColdCell>(),
        size_of::<Cell>(),
        size_of::<Option<Cell>>(),
        size_of::<Slot>(),
        size_of::<Rc<Vec<u64>>>(),
    );
}
