// Declared values materialized when a raw physical region exits.

use PhysicalOutputDestination::{LocalSlot, Register};
use PhysicalOutputValue::{Array, Element, Index, Result};

const ARRAY_GET_OUTPUTS: &[PhysicalOutput] = &[PhysicalOutput {
    value: Element,
    destination: Register(operand(0, A)),
}];

const ARRAY_GET_INC_OUTPUTS: &[PhysicalOutput] = &[
    PhysicalOutput {
        value: Element,
        destination: Register(operand(0, A)),
    },
    PhysicalOutput {
        value: Index,
        destination: Register(operand(0, C)),
    },
];

const ARRAY_UPDATE_OUTPUTS: &[PhysicalOutput] = &[
    PhysicalOutput {
        value: Element,
        destination: Register(operand(0, A)),
    },
    PhysicalOutput {
        value: Result,
        destination: Register(operand(1, A)),
    },
];

const ARRAY_LOOP_BODY_OUTPUTS: &[PhysicalOutput] = &[
    PhysicalOutput {
        value: Array,
        destination: Register(operand(0, A)),
    },
    PhysicalOutput {
        value: Element,
        destination: Register(operand(1, A)),
    },
    PhysicalOutput {
        value: Result,
        destination: Register(operand(2, A)),
    },
];

const ARRAY_NUMERIC_LOOP_OUTPUTS: &[PhysicalOutput] = &[
    PhysicalOutput {
        value: Index,
        destination: LocalSlot(operand(17, B)),
    },
    PhysicalOutput {
        value: Array,
        destination: Register(operand(13, A)),
    },
    PhysicalOutput {
        value: Result,
        destination: Register(operand(12, A)),
    },
    PhysicalOutput {
        value: Result,
        destination: Register(operand(14, A)),
    },
    PhysicalOutput {
        value: Index,
        destination: Register(operand(0, A)),
    },
];
