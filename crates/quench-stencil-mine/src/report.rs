//! Text and JSON rendering of the ranked table and the recommended set.
use crate::rank::Ranked;
use crate::recommend::Budget;
use quench_runtime::units::PERCENT;
use serde_json::{Value, json};

pub struct Report<'a> {
    pub weight_source: &'static str,
    pub total_dynamic: f64,
    pub static_instructions: usize,
    pub library_rows: usize,
    pub budget: &'a Budget,
    pub ranked: &'a [Ranked],
    pub recommended: &'a [Ranked],
}

fn row_text(rank: usize, row: &Ranked) -> String {
    format!(
        "{rank:>4}  {:<8} {:>5} {:>14.0} {:>8.3}% {:>14.0} {:>10}  {}",
        row.shape.name(), row.static_sites, row.dynamic, row.covered_share * PERCENT, row.dispatches_saved, row.variants,
        row.pattern.render()
    )
}

fn table_text(title: &str, rows: &[Ranked], limit: usize) -> String {
    let header = format!(
        "{:>4}  {:<8} {:>5} {:>14} {:>9} {:>14} {:>10}  pattern\n",
        "#", "shape", "sites", "dynamic", "covered", "saved", "variants"
    );
    let body: Vec<String> = rows.iter().take(limit).enumerate().map(|(i, row)| row_text(i + 1, row)).collect();
    format!("{title}\n{header}{}\n", body.join("\n"))
}

impl Report<'_> {
    pub fn text(&self, top: usize) -> String {
        let totals: f64 = self.recommended.iter().map(|row| row.dispatches_saved).sum();
        let variants: u64 = self.recommended.iter().map(|row| row.variants).sum();
        format!(
            "weights: {}  executed instructions: {:.0}  static instructions: {}  bjit library rows: {}\n\n{}\n{}\nrecommended: {} supernodes (max {}), {} stencil variants (max {}), {:.0} dispatches saved ({:.3}% of executed)\n",
            self.weight_source, self.total_dynamic, self.static_instructions, self.library_rows,
            table_text("ranked candidates", self.ranked, top),
            table_text("recommended set", self.recommended, self.recommended.len()),
            self.recommended.len(), self.budget.supernodes, variants, self.budget.variants, totals,
            totals / self.total_dynamic.max(1.0) * PERCENT,
        )
    }

    pub fn json(&self, top: usize) -> Value {
        let row = |row: &Ranked| {
            json!({
                "shape": row.shape.name(), "pattern": row.pattern.render(), "nodes": row.nodes,
                "static_sites": row.static_sites, "dynamic": row.dynamic,
                "covered_share": row.covered_share, "dispatches_saved": row.dispatches_saved,
                "variants": row.variants,
            })
        };
        json!({
            "kind": "quench-stencil-mine", "weights": self.weight_source,
            "executed_instructions": self.total_dynamic, "static_instructions": self.static_instructions,
            "library_rows": self.library_rows,
            "budget": {"supernodes": self.budget.supernodes, "variants": self.budget.variants},
            "ranked": self.ranked.iter().take(top).map(row).collect::<Vec<_>>(),
            "recommended": self.recommended.iter().map(row).collect::<Vec<_>>(),
        })
    }
}
