# Initial CallSite integration: failed qualification

Frozen source 41e9b9f03 passed workspace checking, 647 Core tests, focused CallSite identity/strict visibility/method descriptor comparisons, 43 framework fixtures and nine no-JIT web correctness targets. It did NOT pass the complete qualification.

The quiet serial Node v26.12.0 API run exits 101: 340 pass / 3 fail. assertion_exception_objects_share_strict_property_comparison, children_inherit_directory_identity_after_rename_and_unlink and deep_comparison_modes_match_node_values_keys_and_prototypes all panic at dispatch_fast.rs's returning-call activation-caller expectation. CLI, runner and test hashes are unchanged across this failed run.

The fresh all-eight guard attempts 32 captures, but candidate Earley-Boyer, RayTrace and Richards exit 1 without their completion marker at the same panic. Identities stay fixed. all_valid=false and instruction_limit_met=null: no instruction deltas or speed claim may be inferred from incomplete work.

The new Node proper-tail-call policy had accidentally disabled the specialized boundary fallback even when there was no activation caller. 53ba17cbd moves that policy check inside the existing-caller branch, preserving the general-frame return boundary. Subsequent focused and full verification must be recorded separately. This snapshot was not published. No total parity or web win.
