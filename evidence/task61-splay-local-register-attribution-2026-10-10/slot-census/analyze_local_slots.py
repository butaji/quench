#!/usr/bin/env python3
"""Derive Splay local-slot execution deltas from captured profile stderr."""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent
RECORDS = {
    "baseline_setup": "unpromoted-setup-verified",
    "baseline_setup_run": "unpromoted-setup-run-verified",
    "promoted_setup": "promoted-setup-verified",
    "promoted_setup_run": "promoted-setup-run-verified",
}
FUNCTION_NAMES = {
    6: "GeneratePayloadTree",
    8: "InsertNewNode",
    11: "SplayRun",
    31: "SplayTree.insert",
    32: "SplayTree.remove",
    33: "SplayTree.find",
    34: "SplayTree.findMax",
    35: "SplayTree.findGreatestLessThan",
    38: "SplayTree.splay_",
    39: "SplayTree.Node",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(name: str) -> tuple[dict, dict]:
    lines = (ROOT / f"{name}.stderr").read_text().splitlines()
    census = None
    slots = None
    for line in lines:
        # The first diagnostic build serialized Option<u16> with Rust Debug;
        # normalize only that field while reading its otherwise valid JSON.
        line = re.sub(r'("promoted_register":)Some\((\d+)\)', r"\1\2", line)
        line = line.replace('"promoted_register":None', '"promoted_register":null')
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("kind") == "quench-dispatch-opcode-census":
            census = value
        elif value.get("kind") == "quench-local-slot-census":
            slots = value
    if census is None or slots is None:
        raise ValueError(f"missing census record in {name}")
    local_counts = {"LoadLocalPlain": 0, "StoreLocalPlain": 0}
    for function in slots["functions"]:
        for local in function["locals"]:
            local_counts["LoadLocalPlain"] += local["load_plain"]
            local_counts["StoreLocalPlain"] += local["store_plain"]
    for opcode, total in local_counts.items():
        if total != census["counts"][opcode]:
            raise AssertionError((name, opcode, total, census["counts"][opcode]))
    if census["total"] != census["site_total"]:
        raise AssertionError((name, "physical/site total mismatch"))
    return census, slots


def subtract_slots(after: dict, before: dict) -> dict[int, dict]:
    previous = {function["id"]: function for function in before["functions"]}
    result = {}
    for function in after["functions"]:
        prior = previous.get(function["id"], {"locals": []})
        prior_slots = {local["slot"]: local for local in prior["locals"]}
        locals_delta = []
        for local in function["locals"]:
            old = prior_slots.get(local["slot"], {})
            locals_delta.append(
                {
                    **local,
                    "load_plain": local["load_plain"] - old.get("load_plain", 0),
                    "store_plain": local["store_plain"] - old.get("store_plain", 0),
                }
            )
        result[function["id"]] = {**function, "locals": locals_delta}
    return result


def subtract_counts(after: dict, before: dict) -> dict[str, int]:
    return {
        opcode: count - before["counts"].get(opcode, 0)
        for opcode, count in after["counts"].items()
    }


def main() -> None:
    captured = {key: capture(name) for key, name in RECORDS.items()}
    baseline_ops = subtract_counts(captured["baseline_setup_run"][0], captured["baseline_setup"][0])
    promoted_ops = subtract_counts(captured["promoted_setup_run"][0], captured["promoted_setup"][0])
    baseline_slots = subtract_slots(captured["baseline_setup_run"][1], captured["baseline_setup"][1])
    promoted_slots = subtract_slots(captured["promoted_setup_run"][1], captured["promoted_setup"][1])
    promoted_function_rows = {
        function_id: function for function_id, function in promoted_slots.items()
    }

    promoted_by_function = {
        function["id"]: {
            local["slot"]: local["promoted_register"]
            for local in function["locals"]
            if local["promoted_register"] is not None
        }
        for function in captured["promoted_setup_run"][1]["functions"]
    }
    parameter_promoted = [0, 0]
    parameter_unpromoted = [0, 0]
    non_parameter = [0, 0]
    function_rows = []
    for function_id, function in sorted(baseline_slots.items()):
        promoted_slots = promoted_by_function.get(function_id, {})
        categories = {
            "promoted_parameter": [0, 0],
            "unpromoted_parameter": [0, 0],
            "non_parameter_local": [0, 0],
        }
        local_rows = []
        for local in function["locals"]:
            counts = [local["load_plain"], local["store_plain"]]
            if local["slot"] in promoted_slots:
                category = "promoted_parameter"
                parameter_promoted[0] += counts[0]
                parameter_promoted[1] += counts[1]
            elif local["parameter"]:
                category = "unpromoted_parameter"
                parameter_unpromoted[0] += counts[0]
                parameter_unpromoted[1] += counts[1]
            else:
                category = "non_parameter_local"
                non_parameter[0] += counts[0]
                non_parameter[1] += counts[1]
            categories[category][0] += counts[0]
            categories[category][1] += counts[1]
            candidate_function = promoted_function_rows.get(function_id)
            candidate_locals = (
                {row["slot"]: row for row in candidate_function["locals"]}
                if candidate_function
                else {}
            )
            candidate_local = candidate_locals.get(local["slot"])
            local_rows.append(
                {
                    "slot": local["slot"],
                    "name": local["name"],
                    "parameter": local["parameter"],
                    "promotion_register": promoted_slots.get(local["slot"]),
                    "category": category,
                    "baseline_run": {
                        "load_plain": counts[0],
                        "store_plain": counts[1],
                    },
                    "promoted_run": {
                        "load_plain": candidate_local["load_plain"] if candidate_local else 0,
                        "store_plain": candidate_local["store_plain"] if candidate_local else 0,
                    },
                }
            )
        baseline_total = sum(row["load_plain"] + row["store_plain"] for row in function["locals"])
        candidate_function = promoted_function_rows.get(function_id)
        candidate_total = (
            sum(row["load_plain"] + row["store_plain"] for row in candidate_function["locals"])
            if candidate_function
            else 0
        )
        if baseline_total or promoted_slots:
            function_rows.append(
                {
                    "function_id": function_id,
                    "name": FUNCTION_NAMES.get(function_id, function["name"]),
                    "baseline_local_dispatches": baseline_total,
                    "promoted_local_dispatches": candidate_total,
                    "promoted_parameter_dispatches_removed": sum(categories["promoted_parameter"]),
                    "residual_local_dispatches": sum(categories["unpromoted_parameter"])
                    + sum(categories["non_parameter_local"]),
                    "slots": local_rows,
                }
            )

    baseline_local = [baseline_ops["LoadLocalPlain"], baseline_ops["StoreLocalPlain"]]
    promoted_local = [promoted_ops["LoadLocalPlain"], promoted_ops["StoreLocalPlain"]]
    removed = [baseline_local[i] - promoted_local[i] for i in range(2)]
    physical_baseline = captured["baseline_setup_run"][0]["total"] - captured["baseline_setup"][0]["total"]
    physical_promoted = captured["promoted_setup_run"][0]["total"] - captured["promoted_setup"][0]["total"]
    move_reduction = baseline_ops.get("Move", 0) - promoted_ops.get("Move", 0)
    expected_remaining = sum(non_parameter) + sum(parameter_unpromoted)
    if sum(removed) != sum(parameter_promoted):
        raise AssertionError("slot promotion map does not reconcile with opcode delta")
    if expected_remaining != sum(promoted_local):
        raise AssertionError("promoted local-op total does not reconcile with residual slot traffic")

    scripts = {
        "setup_only": Path("tasks/evidence/task61-gc-headroom-2026-10-09/splay-setup_only.js"),
        "setup_plus_one_run": Path(
            "tasks/evidence/task61-splay-profile-program-accounting-2026-10-09/splay-profile-one.js"
        ),
    }
    result = {
        "schema": 1,
        "kind": "task61-splay-local-register-promotion-coverage-census",
        "measurement_role": "counter-only compiler-mode attribution; no timing or performance claim",
        "source_revision": "7dfe2e1f1",
        "source_tree_dirty": True,
        "profile_binary": "target/pinned/b2600310b6f404eeac016ac7944c28209600733a77be27db27efc5e6567e4a02/quench-node",
        "profile_binary_sha256": sha256(
            Path("target/pinned/b2600310b6f404eeac016ac7944c28209600733a77be27db27efc5e6567e4a02/quench-node")
        ),
        "features": ["profile-aggregate", "profile-memory"],
        "compile_modes": {
            "baseline": "QUENCH_DISABLE_LOCAL_PROMOTION=1 (local operand forwarding and all other source state retained)",
            "candidate": "local parameter promotion enabled (environment variable unset)",
        },
        "workload": {
            "fixture": "Splay",
            "setup_items": 8000,
            "run_items": 80,
            "run_count": 1,
            "difference_method": "setup-plus-one-run profile minus setup-only profile, separately in each compile mode",
            "scripts": {key: {"path": str(path), "sha256": sha256(path)} for key, path in scripts.items()},
        },
        "capture_hashes": {
            key: {
                "stderr_sha256": sha256(ROOT / f"{name}.stderr"),
                "stdout_sha256": sha256(ROOT / f"{name}.stdout"),
            }
            for key, name in RECORDS.items()
        },
        "counter_reconciliation": {
            "dispatch_total_equals_site_total": True,
            "local_slot_rows_equal_global_opcode_counts": True,
            "promotion_map_equals_local_opcode_delta": True,
            "promoted_candidate_residual_equals_unpromoted_local_slots": True,
        },
        "one_run": {
            "physical_dispatches": {"baseline": physical_baseline, "promoted": physical_promoted},
            "physical_dispatch_reduction": physical_baseline - physical_promoted,
            "physical_dispatch_reduction_percent": (physical_baseline - physical_promoted) / physical_baseline * 100,
            "opcode_reductions": {
                "LoadLocalPlain": removed[0],
                "StoreLocalPlain": removed[1],
                "Move": move_reduction,
            },
            "plain_local_dispatches": {
                "baseline": {"LoadLocalPlain": baseline_local[0], "StoreLocalPlain": baseline_local[1], "total": sum(baseline_local)},
                "promoted": {"LoadLocalPlain": promoted_local[0], "StoreLocalPlain": promoted_local[1], "total": sum(promoted_local)},
                "promoted_parameter_removed": {"LoadLocalPlain": removed[0], "StoreLocalPlain": removed[1], "total": sum(removed)},
                "promoted_parameter_share_of_baseline_percent": sum(removed) / sum(baseline_local) * 100,
                "promoted_parameter_share_of_old_82957_count_percent": sum(removed) / 82957 * 100,
                "unpromoted_parameter": {"LoadLocalPlain": parameter_unpromoted[0], "StoreLocalPlain": parameter_unpromoted[1], "total": sum(parameter_unpromoted)},
                "non_parameter_locals": {"LoadLocalPlain": non_parameter[0], "StoreLocalPlain": non_parameter[1], "total": sum(non_parameter)},
                "residual_share_of_baseline_percent": sum(promoted_local) / sum(baseline_local) * 100,
            },
            "details_by_function_and_slot": function_rows,
        },
        "historical_denominator_note": {
            "saved_pre_promotion_count": 82957,
            "current_unpromoted_count": sum(baseline_local),
            "difference": 82957 - sum(baseline_local),
            "note": "The current dirty tree includes the local-operand-forwarding candidate in both compile modes. Its separate evidence anticipated 10,000 fewer LoadLocalPlain dispatches/run; this same-source mode comparison, rather than the older denominator, is the basis for promotion coverage. The remaining one-dispatch difference versus that anticipated figure is not isolated here.",
        },
        "performance_claim": None,
    }
    (ROOT / "promotion-coverage.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({k: result["one_run"][k] for k in ["physical_dispatches", "physical_dispatch_reduction", "physical_dispatch_reduction_percent", "opcode_reductions", "plain_local_dispatches"]}, indent=2))


if __name__ == "__main__":
    main()
