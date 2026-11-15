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
        let seq = sequential(&mut nodes);
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

    /// The body's stores, one per array.
    fn stores(&mut self, graph: &Graph) -> Option<Vec<Store>> {
        let stores = graph
            .stores
            .iter()
            .map(|&(base, index, value, order)| {
                Some(Store { base: self.base(base)?, index: self.affine(index)?, value: self.arg(value)?, order })
            })
            .collect::<Option<Vec<Store>>>()?;
        distinct_stores(&stores).then_some(stores)
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

/// One store per base register: a second store to the same array would need its order.
fn distinct_stores(stores: &[Store]) -> bool {
    stores.iter().enumerate().all(|(i, store)| stores[..i].iter().all(|other| other.base != store.base))
}

/// The nodes that read a recurrence, directly or through another node, in body order.
fn sequential(nodes: &mut [Node]) -> Vec<u16> {
    for id in 0..nodes.len() {
        let node = nodes[id];
        let reads = |arg: Arg| match arg {
            Arg::Phi(_) => true,
            Arg::Col(n) => nodes[usize::from(n)].seq,
            _ => false,
        };
        let seq = matches!(node.op, NodeOp::Arith(_) | NodeOp::Bits(_)) && (reads(node.a) || reads(node.b));
        nodes[id].seq = seq;
    }
    (0..nodes.len() as u16).filter(|&id| nodes[usize::from(id)].seq).collect()
}
