# Legacy Cell access path audit — M4 source review

Date: 2026-10-11. This is a source-path audit for candidate worktree
`/private/tmp/quench-task61-record-current` (HEAD `daea35418f5b03fb0f5d2b292fd4a2e0db366d34` plus its current working-tree diff). It is not a machine-instruction attribution or a performance measurement.

## Finding

The proposed fix “reserve a contiguous range for legacy cells” is already present in this candidate. `RecordArena<T>` reserves one `MaybeUninit<T>` range and resolves an ID with `base + id`; its block directory and occupancy bitmap are for allocation, liveness bookkeeping, sweep, and drop. The production lookup `RecordArena::get_live` does not consult either structure. Its `is_occupied` call is only in `debug_assert!`, and the production profile explicitly sets `debug-assertions = false` (`Cargo.toml:68–82`). `RecordArena::get` does walk the directory and check occupancy, but it is `#[cfg(test)]` only (`heap/records.rs:480–490`); it is not the runtime path.

The source-level production path for a legacy value is instead:

1. `Heap::get` calls `slot_entry(value)` and then `SlotEntry::cell_record_id` (`heap/access.rs:108–112`).
2. `slot_entry` decodes the heap reference and rejects direct-object handles; `slot_entry_at` bounds-checks the legacy slot index, loads its 32-bit `SlotEntry`, and tests its nonzero/live tag (`heap/access.rs:173–188`).
3. `cell_record_id` checks the entry kind and shifts out the record ID (`heap/slots.rs:50–55`).
4. `RecordArena::get_live` performs the direct base-plus-ID load; the occupancy verification is debug-only (`heap/records.rs:492–508`).

For comparison, trunk `81c514727`'s `Heap::get` decodes the heap index, bounds-checks it, and reads `Slot.cell.as_ref()` directly (`git show 81c514727:crates/quench-runtime/src/heap/access.rs:107–113`). Thus the source difference to investigate is the stable slot-to-record indirection and its tag/liveness checks, not a per-access block-directory walk. This audit does not quantify the generated instruction delta or establish that these checks explain the Crypto/NavierStokes regression.

## Consequence for the next experiment

Do not implement a second “contiguous legacy arena” as the next fix: the candidate already has one. The discriminating runtime experiment should remove or bypass the `SlotEntry` mapping for legacy values (or route arrays into their own direct-ID record space), then use the schema-5 fixed-work M4 gate. That is a design candidate only; the source audit does not choose between those representations.
