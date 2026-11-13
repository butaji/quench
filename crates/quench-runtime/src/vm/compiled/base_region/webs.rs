use super::*;

/// A register's webs: its definitions joined whenever one value of it is live where several
/// reach, so each web holds one value lifetime and can take its own representation and home.
/// Rows are renamed to web numbers (`Webs::split`); `orig` maps a web back to its register,
/// whose frame slot a web written back at an exit or kept in memory uses.
///
/// The analysis is flat arrays: the registers the rows name are columns, the definitions of a
/// column the bits of a state, one state per row and per branch target outside the region.
pub(super) struct Webs {
    pub(super) orig: Vec<Register>,
    /// The registers the rows name, ascending: the columns of `at` and `leaf_at`.
    named: Vec<Register>,
    /// Per row and column: the web of the definition reaching the row (`NO_WEB` at a row control
    /// never reaches).
    at: Vec<WebId>,
    /// The branch targets outside the region, ascending, and per target and column the web
    /// reaching it.
    leaf_pcs: Vec<Pc>,
    leaf_at: Vec<WebId>,
}

/// A bytecode PC, and a web's number (the register number a renamed row names it by).
type Pc = u32;
type WebId = Register;
const NO_WEB: WebId = WebId::MAX;

/// A set of small numbers, one bit each.
#[derive(Default)]
pub(super) struct Bits(Vec<u64>);

impl Bits {
    pub(super) fn insert(&mut self, member: usize) {
        if self.0.len() <= member / WORD {
            self.0.resize(member / WORD + 1, 0);
        }
        self.0[member / WORD] |= 1 << (member % WORD);
    }

    fn contains(&self, member: usize) -> bool {
        self.0.get(member / WORD).is_some_and(|word| word >> (member % WORD) & 1 != 0)
    }
}

/// Where control goes after a row: a row of the region, or a PC outside it (which includes
/// falling off its last row).
#[derive(Clone, Copy)]
pub(super) enum Next {
    Row(usize),
    Leave(Pc),
}

/// The row of the span at `pc`, if it is inside.
fn row_at(span: &Span, pc: Pc) -> Option<usize> {
    (span.header..=span.backedge).contains(&pc).then(|| (pc - span.header) as usize)
}

impl Decoded {
    /// The registers the row reads.
    fn reads(&self) -> impl Iterator<Item = Register> {
        [self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
    }

    /// Where control goes after row `k` of `count`.
    pub(super) fn successors(&self, k: usize, count: usize, region: &LoopRegion) -> impl Iterator<Item = Next> {
        let fall = (self.kind != Kind::Jump && !self.completes && self.step.is_none_or(Step::falls)).then(|| if k + 1 < count { Next::Row(k + 1) } else { Next::Leave(region.backedge + 1) });
        let branch = self.target.map(|target| row_at(region, target).map_or(Next::Leave(target), Next::Row));
        fall.into_iter().chain(branch)
    }
}

/// A definition of a named register: its column, and its number in it. Number 0 is the value the
/// region is entered with, then one per row that writes the register, in row order.
#[derive(Clone, Copy)]
struct Def {
    column: usize,
    local: usize,
}

/// The definitions of the named registers, laid out as bit columns: column `j` owns the words
/// `words[j]..words[j + 1]` of a state and the union-find nodes `nodes[j]..nodes[j + 1]`.
struct Defs {
    named: Vec<Register>,
    /// The definitions each row makes: its result, and the register a step writes beside it
    /// (`Decoded::also`).
    written: Vec<[Option<Def>; STEP_WRITES]>,
    nodes: Vec<usize>,
    words: Vec<usize>,
}

/// The running totals of `sizes`, from 0.
fn prefix(sizes: impl Iterator<Item = usize>) -> Vec<usize> {
    std::iter::once(0).chain(sizes.scan(0, |total, size| { *total += size; Some(*total) })).collect()
}

/// The positions of the set bits of `word`, ascending.
fn bits_of(mut word: u64) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        (word != 0).then(|| {
            let bit = word.trailing_zeros() as usize;
            word &= word - 1;
            bit
        })
    })
}

impl Defs {
    fn of(decoded: &[Decoded]) -> Self {
        let mut named: Vec<Register> = decoded.iter().flat_map(|row| row.registers()).collect();
        named.sort_unstable();
        named.dedup();
        let mut counts = vec![1usize; named.len()];
        let written = decoded
            .iter()
            .map(|row| {
                [row.dst, row.also].map(|dst| {
                    dst.map(|dst| {
                        let column = named.binary_search(&dst).expect("a named register");
                        counts[column] += 1;
                        Def { column, local: counts[column] - 1 }
                    })
                })
            })
            .collect();
        let (nodes, words) = (prefix(counts.iter().copied()), prefix(counts.iter().map(|count| count.div_ceil(WORD))));
        Self { named, written, nodes, words }
    }

    fn columns(&self) -> usize {
        self.named.len()
    }

    /// The words of a state.
    fn width(&self) -> usize {
        self.words[self.columns()]
    }

    /// The union-find nodes: one per definition.
    fn total(&self) -> usize {
        self.nodes[self.columns()]
    }

    fn column_of(&self, register: Register) -> usize {
        self.named.binary_search(&register).expect("a named register")
    }

    fn node(&self, def: Def) -> usize {
        self.nodes[def.column] + def.local
    }

    /// The definitions of `column` that `state` holds, in order.
    fn held<'a>(&'a self, state: &'a [u64], column: usize) -> impl Iterator<Item = Def> + 'a {
        let words = &state[self.words[column]..self.words[column + 1]];
        words.iter().enumerate().flat_map(move |(at, word)| bits_of(*word).map(move |bit| Def { column, local: at * WORD + bit }))
    }
}

/// `into` united with `from`; whether it grew.
fn merge(into: &mut [u64], from: &[u64]) -> bool {
    let mut grew = false;
    for (word, extra) in into.iter_mut().zip(from) {
        let before = *word;
        *word |= extra;
        grew |= *word != before;
    }
    grew
}

/// The definitions reaching each row and each branch target outside the region: states of
/// `width` words, row `k`'s at `k * width`.
struct Flow {
    width: usize,
    at: Vec<u64>,
    leaf_pcs: Vec<Pc>,
    leaves: Vec<u64>,
}

/// The branch targets outside the region, ascending.
fn leave_targets(span: &Span, decoded: &[Decoded]) -> Vec<Pc> {
    let count = decoded.len();
    let outside = decoded.iter().enumerate().flat_map(|(k, row)| row.successors(k, count, span));
    let mut pcs: Vec<Pc> = outside.filter_map(|next| if let Next::Leave(pc) = next { Some(pc) } else { None }).collect();
    pcs.sort_unstable();
    pcs.dedup();
    pcs
}

impl Flow {
    fn row(&self, k: usize) -> &[u64] {
        &self.at[k * self.width..(k + 1) * self.width]
    }

    fn leaf(&self, k: usize) -> &[u64] {
        &self.leaves[k * self.width..(k + 1) * self.width]
    }

    /// Merges `out`, the state after a row, into where control goes next; a row whose state grew
    /// is queued.
    fn deliver(&mut self, next: Next, out: &[u64], work: &mut Vec<usize>) {
        let width = self.width;
        match next {
            Next::Row(row) => {
                if merge(&mut self.at[row * width..(row + 1) * width], out) {
                    work.push(row);
                }
            }
            Next::Leave(pc) => {
                let leaf = self.leaf_pcs.binary_search(&pc).expect("a collected leave target");
                merge(&mut self.leaves[leaf * width..(leaf + 1) * width], out);
            }
        }
    }

    /// Propagates the states of the rows in `work` to their successors until nothing grows.
    fn run(&mut self, span: &Span, decoded: &[Decoded], defs: &Defs, work: &mut Vec<usize>) {
        let mut out = vec![0; self.width];
        while let Some(k) = work.pop() {
            out.copy_from_slice(self.row(k));
            for def in defs.written[k].into_iter().flatten() {
                out[defs.words[def.column]..defs.words[def.column + 1]].fill(0);
                out[defs.words[def.column] + def.local / WORD] |= 1 << (def.local % WORD);
            }
            for next in decoded[k].successors(k, decoded.len(), span) {
                self.deliver(next, &out, work);
            }
        }
    }

    /// The first row no definition reaches: every row control reaches holds the entry definition
    /// of each named register, so an empty state is a row control never reaches.
    fn unreached(&self, count: usize) -> Option<usize> {
        (self.width > 0).then(|| (0..count).find(|k| self.row(*k).iter().all(|word| *word == 0))).flatten()
    }

    /// A row control never reaches (it follows a jump, or an instruction that is a leave) is taken
    /// as entered with the region's entry values: its webs are never run, but they must exist.
    fn of(span: &Span, decoded: &[Decoded], defs: &Defs) -> Self {
        let (count, width) = (decoded.len(), defs.width());
        let leaf_pcs = leave_targets(span, decoded);
        let mut flow = Self { width, at: vec![0; count * width], leaves: vec![0; leaf_pcs.len() * width], leaf_pcs };
        // Every named register is entered with its first definition.
        for column in 0..defs.columns() {
            flow.at[defs.words[column]] |= 1;
        }
        let entry = flow.row(0).to_vec();
        let mut work = vec![0];
        loop {
            flow.run(span, decoded, defs, &mut work);
            let Some(k) = flow.unreached(count) else { return flow };
            flow.at[k * width..(k + 1) * width].copy_from_slice(&entry);
            work.push(k);
        }
    }
}

/// The union-find of definitions: each node a definition, numbered by `Defs::node`.
struct Sets {
    parent: Vec<usize>,
}

impl Sets {
    fn root(&mut self, mut node: usize) -> usize {
        while self.parent[node] != node {
            self.parent[node] = self.parent[self.parent[node]];
            node = self.parent[node];
        }
        node
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        self.parent[b] = a;
    }

    /// Joins the definitions of `column` that `state` holds.
    fn join_held(&mut self, defs: &Defs, state: &[u64], column: usize) {
        let mut held = defs.held(state, column).map(|def| defs.node(def));
        let Some(first) = held.next() else { return };
        for other in held {
            self.union(first, other);
        }
    }
}

/// The union-find of definitions where one value of a register is live or read, or the register
/// is left whole (`merged`).
fn joined(span: &Span, decoded: &[Decoded], merged: &Bits, defs: &Defs, flow: &Flow) -> Sets {
    let mut sets = Sets { parent: (0..defs.total()).collect() };
    let whole = |register: Register| merged.contains(usize::from(register));
    for (k, row) in decoded.iter().enumerate() {
        let pc = span.header + k as Pc;
        for (column, register) in defs.named.iter().enumerate() {
            if span.is_live(pc, *register) || row.reads().any(|read| read == *register) || whole(*register) {
                sets.join_held(defs, flow.row(k), column);
            }
        }
    }
    for (leaf, pc) in flow.leaf_pcs.iter().enumerate() {
        for (column, register) in defs.named.iter().enumerate() {
            if span.is_live(*pc, *register) || whole(*register) {
                sets.join_held(defs, flow.leaf(leaf), column);
            }
        }
    }
    for column in (0..defs.columns()).filter(|column| whole(defs.named[*column])) {
        for node in defs.nodes[column] + 1..defs.nodes[column + 1] {
            sets.union(defs.nodes[column], node);
        }
    }
    sets
}

/// Numbers the webs in the order rows first name them.
struct Renamer<'a> {
    defs: &'a Defs,
    flow: &'a Flow,
    sets: Sets,
    /// The web of each union-find root, once named.
    number: Vec<WebId>,
    orig: Vec<Register>,
}

impl Renamer<'_> {
    fn web(&mut self, def: Def) -> WebId {
        let root = self.sets.root(self.defs.node(def));
        if self.number[root] == NO_WEB {
            self.number[root] = WebId::try_from(self.orig.len()).expect("a web number");
            self.orig.push(self.defs.named[def.column]);
        }
        self.number[root]
    }

    /// The web of each column's first definition in `state` (`NO_WEB` where it holds none).
    fn table(&mut self, state: &[u64]) -> Vec<WebId> {
        let defs = self.defs;
        (0..defs.columns()).map(|column| defs.held(state, column).next().map_or(NO_WEB, |def| self.web(def))).collect()
    }

    fn rename(mut self, decoded: &[Decoded]) -> (Webs, Vec<Decoded>) {
        let (defs, flow) = (self.defs, self.flow);
        let (mut at, mut renamed) = (Vec::with_capacity(decoded.len() * defs.columns()), Vec::with_capacity(decoded.len()));
        for (k, row) in decoded.iter().enumerate() {
            let here = self.table(flow.row(k));
            let read = |register: Register| {
                let web = here[defs.column_of(register)];
                assert_ne!(web, NO_WEB, "a definition reaches every read");
                web
            };
            let source = |source: Option<Source>| match source {
                Some(Source::Register(register)) => Some(Source::Register(read(register))),
                other => other,
            };
            let (a, b, base) = (source(row.a), source(row.b), row.base.map(|register| read(register)));
            let identity = row.identity.map(|(register, word)| (read(register), word));
            let [dst, also] = defs.written[k].map(|def| def.map(|def| self.web(def)));
            renamed.push(Decoded { dst, also, a, b, base, identity, ..*row });
            at.extend(here);
        }
        let leaf_at = (0..flow.leaf_pcs.len()).flat_map(|leaf| self.table(flow.leaf(leaf))).collect();
        (Webs { orig: self.orig, named: defs.named.clone(), at, leaf_pcs: flow.leaf_pcs.clone(), leaf_at }, renamed)
    }
}

impl Webs {
    /// The rows renamed to webs. A register in `merged` is one web.
    pub(super) fn split(span: &Span, decoded: &[Decoded], merged: &Bits) -> (Self, Vec<Decoded>) {
        let defs = Defs::of(decoded);
        let flow = Flow::of(span, decoded, &defs);
        let sets = joined(span, decoded, merged, &defs, &flow);
        Renamer { defs: &defs, flow: &flow, number: vec![NO_WEB; defs.total()], sets, orig: Vec::new() }.rename(decoded)
    }

    /// The webs of `register`.
    pub(super) fn count(&self, register: Register) -> usize {
        self.orig.iter().filter(|orig| **orig == register).count()
    }

    /// The webs live at `pc`: in the region, the ones reaching its row; outside, the ones reaching
    /// that branch target.
    pub(super) fn live_at(&self, span: &Span, pc: u32) -> Vec<usize> {
        let columns = self.named.len();
        let table = match row_at(span, pc) {
            Some(k) => self.at.get(k * columns..(k + 1) * columns),
            None => self.leaf_pcs.binary_search(&pc).ok().and_then(|leaf| self.leaf_at.get(leaf * columns..(leaf + 1) * columns)),
        };
        table.map_or_else(Vec::new, |webs| {
            let reaching = self.named.iter().zip(webs).filter(|(register, web)| **web != NO_WEB && span.is_live(pc, **register));
            reaching.map(|(_, web)| usize::from(*web)).collect()
        })
    }

    /// For each web, the webs that may not share its pin: those live where it is live, or where
    /// it is defined.
    pub(super) fn clashes(&self, span: &Span, decoded: &[Decoded]) -> Vec<Vec<usize>> {
        let mut clash: Vec<Vec<usize>> = vec![Vec::new(); self.orig.len()];
        for (k, row) in decoded.iter().enumerate() {
            let mut held = self.live_at(region, region.header + k as Pc);
            held.extend([row.dst, row.also].into_iter().flatten().map(usize::from));
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
    pub(super) fn entry_live(&self, span: &Span) -> Vec<usize> {
        self.live_at(span, span.header)
    }
}
