//! The mined domain as plain data: a supernode candidate is a pattern (opcode labels plus the
//! dataflow links between them) and the sites in the corpus where it occurs.
use quench_runtime::FlowInstruction;
use std::collections::HashMap;

pub const MIN_NGRAM: usize = 2;
pub const MAX_NGRAM: usize = 4;
pub const MAX_TREE_DEPTH: usize = 3;
/// A depth-3 tree of three-operand instructions could name 13 nodes; larger trees are not
/// supernodes worth a stencil and are dropped.
pub const MAX_TREE_NODES: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label {
    pub opcode: u16,
    pub name: &'static str,
    pub operator: Option<&'static str>,
}

impl Label {
    pub fn of(instruction: &FlowInstruction) -> Self {
        Self { opcode: instruction.opcode, name: instruction.name, operator: instruction.operator }
    }

    pub fn render(&self) -> String {
        match self.operator {
            Some(operator) => format!("{}({operator})", self.name),
            None => self.name.to_string(),
        }
    }
}

/// Node `consumer` reads a register node `producer` wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Link {
    pub producer: u8,
    pub consumer: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pattern {
    pub labels: Vec<Label>,
    pub links: Vec<Link>,
}

impl Pattern {
    pub fn render(&self) -> String {
        let labels: Vec<String> = self.labels.iter().map(Label::render).collect();
        let links: Vec<String> =
            self.links.iter().map(|link| format!("{}>{}", link.producer, link.consumer)).collect();
        format!("{} [{}]", labels.join(" ; "), links.join(","))
    }

    pub fn opcodes(&self) -> Vec<u16> {
        self.labels.iter().map(|label| label.opcode).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shape {
    /// Consecutive instructions of one basic block.
    Sequence,
    /// A dataflow tree whose instructions need not be adjacent.
    Tree,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sequence => "sequence",
            Self::Tree => "tree",
        }
    }
}

/// One static occurrence: the function, the pcs it covers, and its estimated executions.
#[derive(Clone, Debug)]
pub struct Site {
    pub function: u32,
    pub pcs: Vec<u32>,
    pub weight: f64,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub shape: Shape,
    pub pattern: Pattern,
    pub sites: Vec<Site>,
}

pub type Mined = HashMap<(Shape, Pattern), Vec<Site>>;

/// Builds the pattern of `nodes` (indices into `block`, ascending): their labels and the
/// producer links among them.
pub fn pattern_of(block: &[FlowInstruction], links_in: &[Vec<usize>], nodes: &[usize]) -> Pattern {
    let position = |index: usize| nodes.iter().position(|&node| node == index);
    let labels = nodes.iter().map(|&node| Label::of(&block[node])).collect();
    let mut links = Vec::new();
    for (consumer, &node) in nodes.iter().enumerate() {
        for &producer in &links_in[node] {
            if let Some(producer) = position(producer) {
                links.push(Link { producer: producer as u8, consumer: consumer as u8 });
            }
        }
    }
    Pattern { labels, links }
}
