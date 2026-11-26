#!/usr/bin/env python3
"""Buckets the self Ir of a callgrind profile (cgself.py output) by cause, per op.

usage: causes.py OPS FILE [FILE...]   (FILE = cgself.py output with -n large)
Prints `cause ir_per_op share` lines. Buckets are matched in order; the first match wins.
"""
import re, sys

BUCKETS = [
    ("compile", r"cranelift|regalloc2|webs|Webs|base_region|osr_loop|lane_region|facts::|typed_ir::|admission|sort::|SmallVec|jit_backend|Lowering|stencil|emit"),
    ("bigint", r"num_bigint|bigint|BigInt|biguint|strict_equal|asIntN|as_int_n"),
    ("typed-element", r"generated_row::<54,|generated_row::<63,|typed_array_get|typed_array_set|is_int32|NativeKind|typed_element|row_index_write|row_index_read|get_index_miss|absent_array_element|plain_typed_element|canonical_index_key|string_element|get_index|load_element|index_chain|typed_array_number|plain_keyed_get|array_index_key|BufferBytes|string_index|object_data|element|Element|dense_array"),
    ("call-return", r"script_call|return_|pop_plain_frame|push_plain_frame|call_cross|LaneStack|activate|frame_|Frame|CallTarget|remember_target|resolve_call"),
    ("alloc-gc", r"sweep|young|Young|heap::|alloc|dealloc|QuenchAlloc|gc::|Gc|collect|malloc|free|realloc|memcpy|memmove|memset"),
    ("field-access", r"proved_getter|getter|generated_row::<55,|generated_row::<30,|get_field|set_field|GetField|SetField|shape|Shape|property"),
    ("arith", r"immediate_binary|generic_binary|binary_values|Binary|numeric|to_number|f64|i32"),
    ("dispatch", r"run_fast_lane|run_general_loop|step_unlisted|drive_lane|run_row|row_jump|fused_rows|operator_row|generated_row|dispatch|protocol_row|loop_jump"),
    ("tier-checks", r"JitMode|tier_policy|memory_policy|js_tier|tier_may"),
    ("compiled-code", r"^0x[0-9a-f]+$"),
]


def main():
    ops = float(sys.argv[1])
    totals = {}
    grand = 0
    for path in sys.argv[2:]:
        for line in open(path):
            m = re.match(r"\s*([\d,]+)\s+[\d.]+%\s+(.*)", line)
            if not m:
                continue
            ir, name = int(m.group(1).replace(",", "")), m.group(2)
            grand += ir
            cause = next((c for c, rx in BUCKETS if re.search(rx, name)), "other")
            totals[cause] = totals.get(cause, 0) + ir
    for cause, ir in sorted(totals.items(), key=lambda kv: -kv[1]):
        print(f"{cause}\t{ir / ops:.0f}\t{100 * ir / grand:.1f}%")
    print(f"total\t{grand / ops:.0f}\t100%")


if __name__ == "__main__":
    main()
