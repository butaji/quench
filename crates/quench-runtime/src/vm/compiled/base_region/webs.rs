use super::*;
use std::collections::BTreeSet;

/// A register's webs: its definitions joined whenever one value of it is live where several
/// reach, so each web holds one value lifetime and can take its own representation and home.
/// Rows are renamed to web numbers (`Webs::split`); `orig` maps a web back to its register,
/// whose frame slot a web written back at an exit or kept in memory uses.
pub(super) struct Webs {
    pub(super) orig: Vec<Register>,
    /// Per row: the web of each register with a definition reaching it.
    at: Vec<BTreeMap<Register, usize>>,
    /// Per branch target outside the region: the web of each register reaching it.
    leaves: BTreeMap<Pc, BTreeMap<Register, WebId>>,
}

/// A definition: row `k` is `k + 1`, the value the region is entered with is `ENTRY`.
const ENTRY: Def = 0;

/// A definition's number, and a bytecode PC.
type Def = u32;
type Pc = u32;
type WebId = usize;
type Reaching = BTreeMap<Register, BTreeSet<Def>>;

struct Sets {
    parent: Vec<usize>,
    index: BTreeMap<(Register, Def), usize>,
}

impl Sets {
    fn node(&mut self, register: Register, def: Def) -> usize {
        let next = self.parent.len();
        let node = *self.index.entry((register, def)).or_insert(next);
        if node == next {
            self.parent.push(next);
        }
        node
    }

    fn root(&mut self, mut node: usize) -> usize {
        while self.parent[node] != node {
            self.parent[node] = self.parent[self.parent[node]];
            node = self.parent[node];
        }
        node
    }

    /// Joins the definitions reaching `state` of each register `joins` holds for.
    fn join_where(&mut self, state: &Reaching, joins: impl Fn(Register) -> bool) {
        for (register, defs) in state {
            if joins(*register) {
                self.join(*register, defs);
            }
        }
    }

    fn join(&mut self, register: Register, defs: &BTreeSet<Def>) {
        let mut nodes = defs.iter().map(|def| self.node(register, *def)).collect::<Vec<_>>().into_iter();
        let Some(first) = nodes.next() else { return };
        for other in nodes {
            let (a, b) = (self.root(first), self.root(other));
            self.parent[b] = a;
        }
    }
}

impl Decoded {
    /// The registers the row reads.
    fn reads(&self) -> impl Iterator<Item = Register> {
        [self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
    }

    /// Where control goes after the row: row indices within the region, and branch targets
    /// leaving it (which includes falling off its last row).
    fn successors(&self, k: usize, count: usize, region: &LoopRegion) -> (Vec<usize>, Vec<Pc>) {
        let (header, backedge) = (region.header, region.backedge);
        let falls = !matches!(self.kind, Kind::Jump | Kind::JumpPoll);
        let (mut inside, mut outside) = (Vec::new(), Vec::new());
        if falls {
            if k + 1 < count { inside.push(k + 1) } else { outside.push(backedge + 1) }
        }
        if let Some(target) = self.target {
            if (header..=backedge).contains(&target) { inside.push((target - header) as usize) } else { outside.push(target) }
        }
        (inside, outside)
    }
}

/// The definitions reaching each row, and each leave target.
fn reaching(region: &LoopRegion, decoded: &[Decoded]) -> (Vec<Reaching>, BTreeMap<Pc, Reaching>) {
    let count = decoded.len();
    let mut at: Vec<Reaching> = vec![Reaching::new(); count];
    let mut leaves: BTreeMap<Pc, Reaching> = BTreeMap::new();
    for row in decoded {
        for register in row.registers() {
            at[0].entry(register).or_default().insert(ENTRY);
        }
    }
    let merge = |into: &mut Reaching, from: &Reaching| {
        let mut grew = false;
        for (register, defs) in from {
            let set = into.entry(*register).or_default();
            let before = set.len();
            set.extend(defs);
            grew |= set.len() != before;
        }
        grew
    };
    let mut work: Vec<usize> = vec![0];
    while let Some(k) = work.pop() {
        let mut out = at[k].clone();
        if let Some(dst) = decoded[k].dst {
            out.insert(dst, BTreeSet::from([k as Def + 1]));
        }
        let (inside, outside) = decoded[k].successors(k, count, region);
        for next in inside {
            if merge(&mut at[next], &out) {
                work.push(next);
            }
        }
        for target in outside {
            merge(leaves.entry(target).or_default(), &out);
        }
    }
    (at, leaves)
}

/// The union-find of definitions where one value of a register is live or read, or the
/// register is left whole.
fn joined(region: &LoopRegion, decoded: &[Decoded], merged: &BTreeSet<Register>, at: &[Reaching], leaves: &BTreeMap<Pc, Reaching>) -> Sets {
    let mut sets = Sets { parent: Vec::new(), index: BTreeMap::new() };
    for (k, state) in at.iter().enumerate() {
        let pc = region.header + k as Pc;
        sets.join_where(state, |register| region.is_live(pc, register) || decoded[k].reads().any(|read| read == register) || merged.contains(&register));
    }
    for (target, state) in leaves {
        sets.join_where(state, |register| region.is_live(*target, register) || merged.contains(&register));
    }
    for register in merged {
        let all: BTreeSet<Def> = sets.index.keys().filter(|(r, _)| r == register).map(|(_, def)| *def).collect();
        sets.join(*register, &all);
    }
    sets
}

/// Numbers the webs in the order rows first name them.
struct Renamer {
    sets: Sets,
    /// The web of each union-find root, once named.
    number: Vec<Option<WebId>>,
    orig: Vec<Register>,
}

impl Renamer {
    fn web(&mut self, register: Register, def: Def) -> WebId {
        let node = self.sets.node(register, def);
        let root = self.sets.root(node);
        if self.number.len() <= root {
            self.number.resize(root + 1, None);
        }
        *self.number[root].get_or_insert_with(|| { self.orig.push(register); self.orig.len() - 1 })
    }

    /// The web of each register reaching `state`.
    fn table(&mut self, state: &Reaching) -> BTreeMap<Register, WebId> {
        state.iter().map(|(register, defs)| (*register, self.web(*register, *defs.iter().next().expect("a reaching definition")))).collect()
    }

    fn rename(mut self, decoded: &[Decoded], at: &[Reaching], leaves: &BTreeMap<Pc, Reaching>) -> (Webs, Vec<Decoded>) {
        let (mut renamed, mut tables) = (Vec::with_capacity(decoded.len()), Vec::with_capacity(decoded.len()));
        for (k, row) in decoded.iter().enumerate() {
            let here = self.table(&at[k]);
            let read = |register: Register| Register::try_from(here[&register]).expect("a web number");
            let source = |source: Option<Source>| match source {
                Some(Source::Register(register)) => Some(Source::Register(read(register))),
                other => other,
            };
            let dst = row.dst.map(|register| self.web(register, k as Def + 1) as Register);
            renamed.push(Decoded { kind: row.kind, dst, a: source(row.a), b: source(row.b), target: row.target, heap: row.heap, base: row.base.map(read), hole: row.hole, cache: row.cache.clone() });
            tables.push(here);
        }
        let leaf_webs = leaves.iter().map(|(target, state)| (*target, self.table(state))).collect();
        (Webs { orig: self.orig, at: tables, leaves: leaf_webs }, renamed)
    }
}

impl Webs {
    /// The rows renamed to webs. A register in `merged` is one web.
    pub(super) fn split(region: &LoopRegion, decoded: &[Decoded], merged: &BTreeSet<Register>) -> (Self, Vec<Decoded>) {
        let (at, leaves) = reaching(region, decoded);
        let sets = joined(region, decoded, merged, &at, &leaves);
        Renamer { sets, number: Vec::new(), orig: Vec::new() }.rename(decoded, &at, &leaves)
    }

    /// The webs of `register`.
    pub(super) fn count(&self, register: Register) -> usize {
        self.orig.iter().filter(|orig| **orig == register).count()
    }

    /// The webs live at `pc`: in the region, the ones reaching its row; outside, the ones reaching
    /// that branch target.
    pub(super) fn live_at(&self, region: &LoopRegion, pc: u32) -> Vec<usize> {
        let state = if (region.header..=region.backedge).contains(&pc) { self.at.get((pc - region.header) as usize) } else { self.leaves.get(&pc) };
        state.map_or_else(Vec::new, |state| state.iter().filter(|(register, _)| region.is_live(pc, **register)).map(|(_, web)| *web).collect())
    }

    /// For each web, the webs that may not share its pin: those live where it is live, or where
    /// it is defined.
    pub(super) fn clashes(&self, region: &LoopRegion, decoded: &[Decoded]) -> Vec<Vec<usize>> {
        let mut clash: Vec<Vec<usize>> = vec![Vec::new(); self.orig.len()];
        for (k, row) in decoded.iter().enumerate() {
            let mut held = self.live_at(region, region.header + k as Pc);
            held.extend(row.dst.map(usize::from));
            for &web in &held {
                clash[web].extend(held.iter().copied().filter(|other| *other != web));
            }
        }
        for list in &mut clash {
            list.sort_unstable();
            list.dedup();
        }
        clash
    }

    /// The webs the region is entered with: those live at the header that reach its first row.
    pub(super) fn entry_live(&self, region: &LoopRegion) -> Vec<usize> {
        self.live_at(region, region.header)
    }
}
