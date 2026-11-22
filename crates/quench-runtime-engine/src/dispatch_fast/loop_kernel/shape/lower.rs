//! Lowering a symbolic pass to a `Shape`.
use super::*;

impl Lowering<'_> {
    /// The shape of `graph`, whose pass ended with the register file `file`.
    pub(super) fn lower(mut self, graph: Graph, file: &[Sym], test: (Sym, NumberRelation, Sym)) -> Option<Shape> {
        // A loop with no array access is a kernel too: its recurrences and temps run in the
        // scalar pass, one Rust loop over the iterations (C-shaped code runs as C).
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for &node in &graph.nodes {
            nodes.push(traced(self.node(node), "a node's operands")?);
        }
        let stores = traced(self.stores(&graph), "a store")?;
        let (counter, relation, bound) = traced(self.test(test), "the loop test")?;
        let inductions = traced(self.inductions(file), "an induction")?;
        let (phis, temps, guarded) = traced(self.finals(file), "a written register's final value")?;
        for affine in &mut self.affines {
            affine.step = inductions.iter().find(|(register, _)| *register == affine.register).map_or(Stride::Const(0), |&(_, step)| step);
        }
        traced(disjoint_stores(&stores, &self.affines).then_some(()), "stores into one array")?;
        mark_dead(&mut nodes, &phis, &temps, &stores);
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
            guarded,
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

    fn inductions(&self, file: &[Sym]) -> Option<Vec<(u16, Stride)>> {
        (0..file.len())
            .filter(|&register| self.induction[register])
            .map(|register| match file[register] {
                Sym::Affine(r, step) if usize::from(r) == register => Some((r, Stride::Const(step))),
                Sym::Strided(r, by, negative) if usize::from(r) == register => Some((r, Stride::Register(by, negative))),
                _ => None,
            })
            .collect()
    }

    /// The recurrences' and the other written registers' values at an iteration's end. A
    /// final value may read another recurrence, which joins the list.
    #[allow(clippy::type_complexity)]
    fn finals(&mut self, file: &[Sym]) -> Option<(Vec<(u16, Arg)>, Vec<(u16, Arg)>, Vec<u16>)> {
        let mut phis = Vec::new();
        while phis.len() < self.phis.len() {
            let register = self.phis[phis.len()];
            phis.push((register, self.arg(file[usize::from(register)])?));
        }
        let mut temps = Vec::new();
        let mut guarded = Vec::new();
        for register in 0..file.len() as u16 {
            let index = usize::from(register);
            if self.written[index] && !self.induction[index] && !self.phis.contains(&register) {
                if file[index] == Sym::Poison {
                    guarded.push(register);
                } else {
                    temps.push((register, self.arg(file[index])?));
                }
            }
        }
        // A temp's value may read a recurrence first met here.
        while phis.len() < self.phis.len() {
            let register = self.phis[phis.len()];
            phis.push((register, self.arg(file[usize::from(register)])?));
        }
        temps.retain(|(register, _)| !self.phis.contains(register));
        Some((phis, temps, guarded))
    }
}

/// Marks the operators no recurrence, temp or store reads, directly or through other nodes.
fn mark_dead(nodes: &mut [Node], phis: &[(u16, Arg)], temps: &[(u16, Arg)], stores: &[Store]) {
    let mut live = vec![false; nodes.len()];
    let mut work: Vec<u16> = phis.iter().chain(temps).map(|&(_, arg)| arg).chain(stores.iter().map(|store| store.value))
        .filter_map(|arg| if let Arg::Col(n) = arg { Some(n) } else { None })
        .collect();
    while let Some(n) = work.pop() {
        if std::mem::replace(&mut live[usize::from(n)], true) {
            continue;
        }
        let node = nodes[usize::from(n)];
        work.extend([node.a, node.b].into_iter().filter_map(|arg| if let Arg::Col(m) = arg { Some(m) } else { None }));
    }
    for (node, live) in nodes.iter_mut().zip(live) {
        node.dead = !live && matches!(node.op, NodeOp::Arith(_) | NodeOp::Bits(_) | NodeOp::Cmp(_) | NodeOp::Iota { .. });
    }
}

/// Whether no two stores through one base register can write one element: their indices
/// step together and their offsets differ modulo the step (`line[i]`, `line[i + 1]` with
/// `i += 2`), so the order the chunk commits them in never shows.
fn disjoint_stores(stores: &[Store], affines: &[Affine]) -> bool {
    let apart = |a: &Store, b: &Store| {
        let (a, b) = (affines[usize::from(a.index)], affines[usize::from(b.index)]);
        let step = a.step.constant().filter(|&step| step != 0);
        a.register == b.register && a.step == b.step && step.is_some_and(|step| (a.offset - b.offset).rem_euclid(step) != 0)
    };
    stores.iter().enumerate().all(|(i, store)| stores[..i].iter().all(|other| other.base != store.base || apart(other, store)))
}

/// The nodes that read a recurrence, directly or through another node, in body order. A
/// recurrence whose value at an iteration's end is a column no recurrence feeds (`p = c`)
/// is that column one iteration late, and one that ends as such a recurrence's old value
/// (`q = p; p = c`) is it two iterations late (`lagged`). A node reading one is a column
/// too, where the column is ready first: a load (loads run first), or a node before it.
fn sequential(nodes: &mut [Node], phis: &[(u16, Arg)]) -> Vec<u16> {
    let mut lagged: Vec<Option<(u16, u8)>> = vec![None; phis.len()];
    loop {
        for id in 0..nodes.len() {
            let node = nodes[id];
            let reads = |arg: Arg| match arg {
                Arg::Phi(p) => !lagged[usize::from(p)].is_some_and(|(n, _)| lag_ready(nodes, n, id)),
                Arg::Col(n) => nodes[usize::from(n)].seq,
                _ => false,
            };
            let seq = !node.dead && matches!(node.op, NodeOp::Arith(_) | NodeOp::Bits(_) | NodeOp::Cmp(_)) && (reads(node.a) || reads(node.b));
            nodes[id].seq = seq;
        }
        let next: Vec<Option<(u16, u8)>> = phis
            .iter()
            .map(|&(_, last)| match last {
                Arg::Col(n) if !nodes[usize::from(n)].seq => Some((n, 1)),
                Arg::Phi(q) => lagged[usize::from(q)].filter(|&(_, depth)| depth < MAX_LAG).map(|(n, depth)| (n, depth + 1)),
                _ => None,
            })
            .collect();
        if next == lagged {
            break;
        }
        lagged = next;
    }
    (0..nodes.len() as u16).filter(|&id| nodes[usize::from(id)].seq).collect()
}

/// `value`, reporting what has no kernel model when it is `None` (`Channel::LoopKernel`).
pub(super) fn traced<T>(value: Option<T>, what: &str) -> Option<T> {
    if value.is_none() && quench_runtime::trace::Channel::LoopKernel.on() {
        eprintln!("loop-kernel:   no model for {what}");
    }
    value
}

/// The most iterations late a lagged recurrence reads its column.
const MAX_LAG: u8 = 4;

/// Whether column `n` is computed before node `reader`'s column (`Runner::columns`).
fn lag_ready(nodes: &[Node], n: u16, reader: usize) -> bool {
    usize::from(n) < reader || matches!(nodes[usize::from(n)].op, NodeOp::Load { .. } | NodeOp::Iota { .. })
}
