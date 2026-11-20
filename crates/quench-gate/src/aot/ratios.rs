//! The AOT ratio ratchet (`crates/quench-tier-corpus/aot-ratios.txt`): per row, the ceiling of
//! the AOT binary's wall-clock time over its C analogue's. Growth past the ceiling (beyond host
//! noise) fails; an improvement lowers the ceiling. A row with no value is written only by
//! `--record`.

use std::collections::BTreeMap;

const HEADER: &str = "# AOT ratio ratchet of `quench-gate aot`: per JetStream row, the ceiling of the AOT binary's
# wall-clock time over its C analogue's. Growth past the ceiling fails; an improvement lowers it.
# A row with no value is written only by `--record`.
";
const COMMENT: char = '#';
/// The growth a ceiling tolerates as host noise (the harness measures 5-15% noise).
const GROWTH_TOLERANCE: f64 = 0.10;
/// Decimal places a ratio is written with.
pub const PRECISION: usize = 2;

/// What settling one ratio against its ceiling did.
pub enum Settled {
    Held,
    Grew { limit: f64 },
    Lowered,
    Recorded,
    Unrecorded,
}

/// The ceilings by row name.
#[derive(Debug, Default)]
pub struct Table(BTreeMap<String, f64>);

impl Table {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = BTreeMap::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with(COMMENT)) {
            let (row, ratio) = line.split_once(' ').ok_or_else(|| format!("`{line}`: expected `row ratio`"))?;
            let ratio: f64 = ratio.trim().parse().map_err(|_| format!("`{ratio}` is not a number"))?;
            rows.insert(row.to_owned(), ratio);
        }
        Ok(Self(rows))
    }

    pub fn render(&self) -> String {
        self.0.iter().map(|(row, ratio)| format!("{row} {ratio:.PRECISION$}\n")).fold(HEADER.to_owned(), |text, line| text + &line)
    }

    /// Settles `ratio` for `row`: held, lowered to it, recorded when `record` asks, or grown past.
    pub fn settle(&mut self, row: &str, ratio: f64, record: bool) -> Settled {
        match self.0.get(row).copied() {
            Some(limit) if ratio > limit * (1.0 + GROWTH_TOLERANCE) => Settled::Grew { limit },
            Some(limit) if ratio < limit => {
                self.0.insert(row.to_owned(), ratio);
                Settled::Lowered
            }
            Some(_) => Settled::Held,
            None if record => {
                self.0.insert(row.to_owned(), ratio);
                Settled::Recorded
            }
            None => Settled::Unrecorded,
        }
    }
}
