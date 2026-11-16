//! The C-parity ratchet (`cparity.txt`): per kernel and tier, the ceilings of what the hottest
//! loop's machine code spends per iteration. Only improvement is written automatically: a
//! ceiling a measurement undercuts is lowered to it. A ceiling is never raised. Fields with no
//! value yet (`-`) and kernels with no row are written only when recording is asked, once.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt::Write as _,
};

use super::classes::{FIELD_COUNT, Fields, field_names};

const UNRECORDED: &str = "-";
/// A row for a kernel whose tier compiles no loop at all.
const LOOPLESS: &str = "none";
const COMMENT: char = '#';

#[derive(Debug, PartialEq, Eq)]
enum Row {
    Loopless,
    Limits([Option<u32>; FIELD_COUNT]),
}

/// The rows by kernel and tier name.
#[derive(Debug, Default)]
pub struct Table(BTreeMap<(String, String), Row>);

/// What settling one value against its ceiling did.
enum Settled {
    Held,
    Grew { limit: u32 },
    Lowered,
    Recorded,
    Unrecorded,
}

/// The outcome of settling a run's measurements: what grew, and how many ceilings moved.
#[derive(Debug, Default)]
pub struct Tally {
    pub failures: Vec<String>,
    pub lowered: usize,
    pub recorded: usize,
    pub unrecorded: usize,
}

impl Tally {
    fn add(&mut self, settled: Settled, what: &str, value: u32) {
        match settled {
            Settled::Held => {}
            Settled::Grew { limit } => self.failures.push(format!("{what}: {value} exceeds its ceiling {limit}")),
            Settled::Lowered => self.lowered += 1,
            Settled::Recorded => self.recorded += 1,
            Settled::Unrecorded => self.unrecorded += 1,
        }
    }
}

fn settle_field(limit: &mut Option<u32>, value: u32, record: bool) -> Settled {
    match *limit {
        Some(ceiling) if value > ceiling => Settled::Grew { limit: ceiling },
        Some(ceiling) if value < ceiling => {
            *limit = Some(value);
            Settled::Lowered
        }
        Some(_) => Settled::Held,
        None if record => {
            *limit = Some(value);
            Settled::Recorded
        }
        None => Settled::Unrecorded,
    }
}

impl Row {
    fn parse(fields: &[&str]) -> Result<Self, String> {
        if fields == [LOOPLESS] {
            return Ok(Self::Loopless);
        }
        let limits: Vec<Option<u32>> = fields
            .iter()
            .map(|field| if *field == UNRECORDED { Ok(None) } else { field.parse().map(Some).map_err(|_| format!("`{field}` is not a number")) })
            .collect::<Result<_, _>>()?;
        limits.try_into().map(Self::Limits).map_err(|_| format!("a row needs {FIELD_COUNT} fields"))
    }

    fn render(&self) -> String {
        match self {
            Self::Loopless => LOOPLESS.to_owned(),
            Self::Limits(limits) => limits.iter().map(|limit| limit.map_or_else(|| UNRECORDED.to_owned(), |limit| limit.to_string())).collect::<Vec<_>>().join(" "),
        }
    }

    /// A row recording a first measurement.
    fn first(measured: Option<Fields>) -> Self {
        measured.map_or(Self::Loopless, |fields| Self::Limits(fields.map(Some)))
    }

    fn settle(&mut self, what: &str, measured: Option<Fields>, record: bool, tally: &mut Tally) {
        match (self, measured) {
            (Self::Loopless, _) => {}
            (Self::Limits(_), None) => tally.failures.push(format!("{what}: no hot loop was compiled, but its ceilings are recorded")),
            (Self::Limits(limits), Some(fields)) => {
                for ((limit, value), name) in limits.iter_mut().zip(fields).zip(field_names()) {
                    let settled = settle_field(limit, value, record);
                    tally.add(settled, &format!("{what} {name}"), value);
                }
            }
        }
    }
}

impl Table {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = BTreeMap::new();
        for line in text.lines().map(|line| line.split(COMMENT).next().unwrap_or_default().trim()).filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [kernel, tier, values @ ..] = fields.as_slice() else {
                return Err(format!("short ratchet line `{line}`"));
            };
            let row = Row::parse(values).map_err(|error| format!("ratchet line `{line}`: {error}"))?;
            rows.insert((kernel.to_string(), tier.to_string()), row);
        }
        Ok(Self(rows))
    }

    pub fn render(&self) -> String {
        let names: Vec<&str> = field_names().collect();
        let mut text = format!(
            "{COMMENT} kernel tier {}: ceilings of the hottest loop's machine code per iteration; `{UNRECORDED}` is not recorded yet, `{LOOPLESS}` has no loop; the gate only lowers them\n",
            names.join(" ")
        );
        for ((kernel, tier), row) in &self.0 {
            let _ = writeln!(text, "{kernel} {tier} {}", row.render());
        }
        text
    }

    /// Settles a kernel's measurement on a tier (`None`: it compiled no hot loop) against its row.
    pub fn settle(&mut self, kernel: &str, tier: &str, measured: Option<Fields>, record: bool, tally: &mut Tally) {
        let what = format!("{kernel} {tier}");
        match self.0.entry((kernel.to_owned(), tier.to_owned())) {
            Entry::Occupied(mut row) => row.get_mut().settle(&what, measured, record, tally),
            Entry::Vacant(slot) if record => {
                slot.insert(Row::first(measured));
                tally.recorded += 1;
            }
            Entry::Vacant(_) => tally.unrecorded += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# header\nsum u-jit 7 - - - - - - -  # seeded\nfib u-jit none\n";

    #[test]
    fn a_table_round_trips_and_keeps_unrecorded_fields() {
        let table = Table::parse(TEXT).expect("parses");
        let rendered = table.render();
        assert!(rendered.contains("sum u-jit 7 - - - - - - -\n") && rendered.contains("fib u-jit none\n"));
        assert_eq!(Table::parse(&rendered).expect("parses").0, table.0);
    }

    #[test]
    fn growth_fails_improvement_lowers_and_new_fields_wait_for_a_record() {
        let mut table = Table::parse(TEXT).expect("parses");
        let mut tally = Tally::default();
        table.settle("sum", "u-jit", Some([8, 0, 0, 0, 0, 0, 0, 0]), false, &mut tally);
        assert_eq!((tally.failures.len(), tally.unrecorded), (1, FIELD_COUNT - 1));
        let mut tally = Tally::default();
        table.settle("sum", "u-jit", Some([5, 0, 0, 0, 0, 0, 0, 0]), true, &mut tally);
        assert_eq!((tally.failures.len(), tally.lowered, tally.recorded), (0, 1, FIELD_COUNT - 1));
        assert!(table.render().contains("sum u-jit 5 0 0 0 0 0 0 0\n"));
    }

    #[test]
    fn a_missing_loop_fails_only_where_ceilings_are_recorded() {
        let mut table = Table::parse(TEXT).expect("parses");
        let mut tally = Tally::default();
        table.settle("sum", "u-jit", None, false, &mut tally);
        table.settle("fib", "u-jit", None, false, &mut tally);
        table.settle("dot", "u-jit", None, false, &mut tally);
        assert_eq!((tally.failures.len(), tally.unrecorded), (1, 1));
    }
}
