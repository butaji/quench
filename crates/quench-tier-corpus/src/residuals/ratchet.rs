//! The ratchet: the counts each kernel and tier currently emit, which may only go down. A line is
//! `kernel tier boxes guards spills calls instructions`; `#` starts a comment. A pair with no line
//! has no ratchet yet and is held to its spec.

use super::{CATEGORY_COUNT, Counts, Kernel, Tier};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Debug, Default)]
pub struct Ratchet(BTreeMap<(String, Tier), Counts>);

impl Ratchet {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut entries = BTreeMap::new();
        for line in text
            .lines()
            .map(|line| line.split('#').next().unwrap_or_default().trim())
            .filter(|line| !line.is_empty())
        {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [kernel, tier, counts @ ..] = fields.as_slice() else {
                return Err(format!("short ratchet line `{line}`"));
            };
            let tier = Tier::from_name(tier).ok_or_else(|| format!("unknown tier in `{line}`"))?;
            let counts: Vec<u32> = counts
                .iter()
                .map(|count| count.parse().map_err(|_| format!("bad count in `{line}`")))
                .collect::<Result<_, _>>()?;
            let counts = Counts(
                counts
                    .try_into()
                    .map_err(|_| format!("`{line}` needs {CATEGORY_COUNT} counts"))?,
            );
            entries.insert((kernel.to_string(), tier), counts);
        }
        Ok(Self(entries))
    }

    /// Whether `kernel` has compiled a loop on `tier` before, which it must keep doing.
    pub fn has(&self, kernel: &Kernel, tier: Tier) -> bool {
        self.0.contains_key(&(kernel.name.to_string(), tier))
    }

    /// What `kernel` may emit on `tier`: its ratchet entry, else its spec.
    pub fn ceiling(&self, kernel: &Kernel, tier: Tier) -> Counts {
        self.0
            .get(&(kernel.name.to_string(), tier))
            .copied()
            .unwrap_or_else(|| kernel.expected(tier))
    }

    /// The ratchet after `measured` (kernel, tier, counts): each entry the lower of its old value
    /// and the measurement, a new entry the measurement.
    pub fn tightened(
        &self,
        measured: impl IntoIterator<Item = (&'static str, Tier, Counts)>,
    ) -> Self {
        let mut entries = self.0.clone();
        for (kernel, tier, counts) in measured {
            let slot = entries.entry((kernel.to_string(), tier)).or_insert(counts);
            *slot = slot.min(counts);
        }
        Self(entries)
    }

    pub fn render(&self) -> String {
        let mut text = String::from(
            "# kernel tier boxes guards spills calls instructions: current values, shrink-only\n",
        );
        for ((kernel, tier), counts) in &self.0 {
            let counts: Vec<String> = counts.0.iter().map(u32::to_string).collect();
            let _ = writeln!(text, "{kernel} {} {}", tier.name(), counts.join(" "));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_round_trips_and_tightening_only_lowers() {
        let ratchet = Ratchet::default().tightened([("k", Tier::UJit, Counts([1, 2, 3, 4, 5]))]);
        let tighter = Ratchet::parse(&ratchet.render()).unwrap().tightened([(
            "k",
            Tier::UJit,
            Counts([9, 1, 3, 4, 6]),
        )]);
        assert_eq!(
            tighter.0[&("k".to_string(), Tier::UJit)],
            Counts([1, 1, 3, 4, 5])
        );
    }
}
