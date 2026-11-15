//! Lowering a symbolic pass to a `Shape`.
use super::*;

impl Lowering<'_> {
    /// The shape of `graph`, whose pass ended with the register file `file`.
    pub(super) fn lower(mut self, graph: Graph, file: &[Sym], test: (Sym, NumberRelation, Sym)) -> Option<Shape> {
        if graph.loads.is_empty() && graph.stores.is_empty() {
            return None;
        }
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for &node in &graph.nodes {
            nodes.push(self.node(node)?);
        }
        let stores = self.stores(&graph)?;
        let (counter, relation, bound) = self.test(test)?;
        let inductions = self.inductions(file)?;
        let (phis, temps) = self.finals(file)?;
        for affine in &mut self.affines {
            affine.step = inductions.iter().find(|(register, _)| *register == affine.register).map_or(0, |&(_, step)| step);
        }
        if !disjoint_stores(&stores, &self.affines) {
            return None;
        }
        let seq = sequential(&mut nodes, &phis);
        Some(Shape {
            counter,
            relation,
            bound,
            scalars: self.scalars,
            affines: self.affines,
            bases: self.bases,
            nodes,
            seq,
            stores,
            loads: graph.loads,
            phis,
            inductions,
            temps,
        })
    }

    /// The body's stores.
    fn stores(&mut self, graph: &Graph) -> Option<Vec<Store>> {
        graph
            .stores
            .iter()
            .map(|&(base, index, value, order)| {
                Some(Store { base: self.base(base)?, index: self.affine(index)?, value: self.arg(value)?, order })
            })
            .collect()
    }

    fn inductions(&self, file: &[Sym]) -> Option<Vec<(u16, i32)>> {
        (0..file.len())
            .filter(|&register| self.induction[register])
            .map(|register| match file[register] {
                Sym::Affine(r, step) if usize::from(r) == register => Some((r, step)),
                _ => None,
            })
            .collect()
    }

    /// The recurrences' and the other written registers' values at an iteration's end. A
    /// final value may read another recurrence, which joins the list.
    #[allow(clippy::type_complexity)]
    fn finals(&mut self, file: &[Sym]) -> Option<(Vec<(u16, Arg)>, Vec<(u16, Arg)>)> {
        let mut phis = Vec::new();
        while phis.len() < self.phis.len() {
            let register = self.phis[phis.len()];
            phis.push((register, self.arg(file[usize::from(register)])?));
        }
        let mut temps = Vec::new();
        for register in 0..file.len() as u16 {
            let index = usize::from(register);
            if self.written[index] && !self.induction[index] && !self.phis.contains(&register) {
                temps.push((register, self.arg(file[index])?));
            }
        }
        // A temp's value may read a recurrence first met here.
        while phis.len() < self.phis.len() {
            let register = self.phis[phis.len()];
            phis.push((register, self.arg(file[usize::from(register)])?));
        }
        temps.retain(|(register, _)| !self.phis.contains(register));
        Some((phis, temps))
    }
}

/// Whether no two stores through one base register can write one element: their indices
/// step together and their offsets differ modulo the step (`line[i]`, `line[i + 1]` with
/// `i += 2`), so the order the chunk commits them in never shows.
fn disjoint_stores(stores: &[Store], affines: &[Affine]) -> bool {
    let apart = |a: &Store, b: &Store| {
        let (a, b) = (affines[usize::from(a.index)], affines[usize::from(b.index)]);
        a.register == b.register && a.step == b.step && a.step != 0 && (a.offset - b.offset).rem_euclid(a.step) != 0
    };
    stores.iter().enumerate().all(|(i, store)| stores[..i].iter().all(|other| other.base != store.base || apart(other, store)))
}

/// The nodes that read a recurrence, directly or through another node, in body order. A
/// recurrence whose value at an iteration's end is a column no recurrence feeds (`p = c`)
/// is that column one iteration late, so a node reading it is a column too (`lagged`),
/// where the column is ready first: a load (loads run first), or a node before the reader.
fn sequential(nodes: &mut [Node], phis: &[(u16, Arg)]) -> Vec<u16> {
    let mut lagged = vec![false; phis.len()];
    loop {
        for id in 0..nodes.len() {
            let node = nodes[id];
            let reads = |arg: Arg| match arg {
                Arg::Phi(p) => !(lagged[usize::from(p)] && lag_ready(nodes, phis[usize::from(p)].1, id)),
                Arg::Col(n) => nodes[usize::from(n)].seq,
                _ => false,
            };
            let seq = matches!(node.op, NodeOp::Arith(_) | NodeOp::Bits(_)) && (reads(node.a) || reads(node.b));
            nodes[id].seq = seq;
        }
        let next: Vec<bool> = phis.iter().map(|&(_, last)| matches!(last, Arg::Col(n) if !nodes[usize::from(n)].seq)).collect();
        if next == lagged {
            break;
        }
        lagged = next;
    }
    (0..nodes.len() as u16).filter(|&id| nodes[usize::from(id)].seq).collect()
}

/// Whether column `last` is computed before node `reader`'s column (`Runner::columns`).
pub(super) fn lag_ready(nodes: &[Node], last: Arg, reader: usize) -> bool {
    match last {
        Arg::Col(n) => usize::from(n) < reader || matches!(nodes[usize::from(n)].op, NodeOp::Load { .. } | NodeOp::Iota { .. }),
        _ => false,
    }
}
