#!/usr/bin/env bash
# Milestone helper: run the stage failure triage flow and optionally advance index.json.
# Usage:
#   bash tools/milestone.sh                    # uses TEST262_STAGE if set, else current_stage
#   bash tools/milestone.sh --stage 32 --advance
#   bash tools/milestone.sh --stage 32 --commit
#   bash tools/milestone.sh --stage 32 --test-run --preflight # preflight + stage test-run
#   bash tools/milestone.sh --commit --push  # uses TEST262_STAGE if set, else current_stage
#   bash tools/milestone.sh --status                   # print stage progress and current stage
#   bash tools/milestone.sh --status --json              # print stage progress as JSON
#   bash tools/milestone.sh --status --ci                  # include CI readiness gate
#   bash tools/milestone.sh --status --ci --json            # include CI readiness gate as JSON
#   bash tools/milestone.sh --status --ci --raw             # include CI readiness gate as compact raw output
#   bash tools/milestone.sh --status --current            # print current stage only
#   bash tools/milestone.sh --status --next                # print next pending stage
#   bash tools/milestone.sh --status --next-id             # print next pending stage id only
#   bash tools/milestone.sh --status --history 20       # show last 20 logged events
#   bash tools/milestone.sh --stage 32 --test-run --commit --push
#   bash tools/milestone.sh --stage 32 --dry-run         # do not mutate state or git
#   env MILESTONE_RERUN_JSON_OUT=/tmp/milestone-rerun.json bash tools/milestone.sh --rerun --rerun-json --stage 32
#   bash tools/milestone.sh --stage 32 --rerun           # auto-rerun first failure on fail
#   bash tools/milestone.sh --stage 32 --rerun --rerun-json # rerun first failure and emit JSON
#   bash tools/milestone.sh --rerun-json --stage 32  # uses env: MILESTONE_RERUN_JSON_OUT
#   bash tools/milestone.sh --stage 32 --rerun --tail 20 # show last 20 logged events
#   bash tools/milestone.sh --log /tmp/milestones.log --status
#   bash tools/milestone.sh --quiet                    # suppress human-readable output
#   bash tools/milestone.sh --ci-gate                 # run CI readiness gate (current + next)
#   bash tools/milestone.sh --ci-gate --ci-gate-json # machine-readable gate payload
#   bash tools/milestone.sh --ci-gate --json          # machine-readable gate payload

set -euo pipefail

cd "$(dirname "$0")/.."

STAGE=""
STATUS_ONLY=0
CI_GATE_ONLY=0
CI_GATE_JSON=0
CI_GATE_SKIP_CURRENT=0
CI_GATE_SKIP_NEXT=0
CI_GATE_RUN=0
CI_GATE_ARGS=()
STATUS_JSON=0
STATUS_CURRENT=0
STATUS_NEXT=0
NEXT_ID_ONLY=0
STATUS_WITH_CI=0
STATUS_RAW=0
RUN_TEST_RUN=0
RUN_TEST_RUN_PREFLIGHT=1
AUTO_ADVANCE=0
AUTO_COMMIT=0
AUTO_PUSH=0
DRY_RUN=0
AUTO_RERUN=0
RERUN_JSON=0
RERUN_JSON_OUT=""
QUIET=0
COMMIT_MESSAGE=""
HISTORY=0
LOG_FILE="${MILESTONE_LOG:-./.test262_milestones.log}"

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --stage)
            STAGE="$2"
            shift 2
            ;;
        --commit)
            AUTO_COMMIT=1
            if [[ ${2:-} != --* && ${2:-} != "" ]]; then
                COMMIT_MESSAGE="$2"
                shift 2
            else
                shift
            fi
            ;;
        --push)
            AUTO_PUSH=1
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --rerun)
            AUTO_RERUN=1
            shift
            ;;
        --rerun-json)
            AUTO_RERUN=1
            RERUN_JSON=1
            QUIET=1
            shift
            ;;
        --rerun-json-out)
            RERUN_JSON_OUT="$2"
            shift 2
            ;;
        --status)
            STATUS_ONLY=1
            shift
            while [[ ${#} -gt 0 ]]; do
                case "${1:-}" in
                    --json)
                        STATUS_JSON=1
                        shift
                        ;;
                    --raw)
                        STATUS_RAW=1
                        shift
                        ;;
                    --ci)
                        STATUS_WITH_CI=1
                        shift
                        ;;
                    --current)
                        STATUS_CURRENT=1
                        shift
                        ;;
                    --next)
                        STATUS_NEXT=1
                        shift
                        ;;
                    --next-id)
                        STATUS_NEXT=1
                        NEXT_ID_ONLY=1
                        shift
                        ;;
                    *)
                        break
                        ;;
                esac
            done
            if [[ "$STATUS_JSON" -eq 1 && "$STATUS_RAW" -eq 1 ]]; then
                # Machine-readable output takes precedence over raw.
                STATUS_RAW=0
            fi
            ;;
        --status-json)
            STATUS_ONLY=1
            STATUS_JSON=1
            shift
            ;;
        --ci-gate)
            CI_GATE_ONLY=1
            shift
            ;;
        --ci-gate-json)
            CI_GATE_ONLY=1
            CI_GATE_JSON=1
            shift
            ;;
        --json)
            CI_GATE_ONLY=1
            CI_GATE_JSON=1
            shift
            ;;
        --skip-current)
            CI_GATE_SKIP_CURRENT=1
            shift
            ;;
        --skip-next)
            CI_GATE_SKIP_NEXT=1
            shift
            ;;
        --run)
            CI_GATE_RUN=1
            shift
            ;;
        --test-run)
            RUN_TEST_RUN=1
            shift
            ;;
        --preflight)
            RUN_TEST_RUN_PREFLIGHT=1
            shift
            ;;
        --advance)
            AUTO_ADVANCE=1
            shift
            ;;
        --history)
            if [[ "${2:-}" == "" ]]; then
                echo "error: --history requires a numeric argument" >&2
                exit 1
            fi
            HISTORY="$2"
            shift 2
            ;;
        --tail)
            if [[ "${2:-}" == "" ]]; then
                echo "error: --tail requires a numeric argument" >&2
                exit 1
            fi
            HISTORY="$2"
            shift 2
            ;;
        --log)
            if [[ "${2:-}" == "" ]]; then
                echo "error: --log requires a file path" >&2
                exit 1
            fi
            LOG_FILE="$2"
            shift 2
            ;;
        --quiet)
            QUIET=1
            shift
            ;;
        -h|--help)
            sed -n '1,140p' "$0"
            exit 0
            ;;
        *)
            if [[ "$CI_GATE_ONLY" -eq 1 || "$CI_GATE_JSON" -eq 1 || "$CI_GATE_SKIP_CURRENT" -eq 1 || "$CI_GATE_SKIP_NEXT" -eq 1 ]]; then
                CI_GATE_ARGS+=("$1")
                shift
                continue
            fi
            echo "error: unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

STAGE="${STAGE:-$(bash tools/current-stage.sh)}"
RERUN_JSON_OUT="${MILESTONE_RERUN_JSON_OUT:-$RERUN_JSON_OUT}"
if [[ "$RERUN_JSON" -eq 1 && "$RERUN_JSON_OUT" == "" ]]; then
    RERUN_JSON_OUT=".milestone-rerun-${STAGE}.json"
fi
export TEST262_STAGE="$STAGE"
export TEST262_QUICK=1

log_status() {
    local outcome="$1"
    local note="$2"
    {
        printf "[%s] stage=%s outcome=%s " \
            "$(date +'%Y-%m-%d %H:%M:%S%z')" \
            "$STAGE" \
            "$outcome"
        printf "branch=%s " "$(git branch --show-current)"
        printf "commit=%s " "$(git rev-parse --short HEAD)"
        printf "note=%s\n" "$note"
    } >> "$LOG_FILE"
}

log_msg() {
    if [[ "$QUIET" -eq 1 ]]; then
        return
    fi
    echo "$1"
}

show_history() {
    if [[ "$HISTORY" =~ ^[0-9]+$ ]] && [[ "$HISTORY" -gt 0 ]]; then
        bash tools/milestone-timeline.sh "$HISTORY" --log "$LOG_FILE"
    fi
}

if [[ "$DRY_RUN" -eq 1 ]]; then
    AUTO_COMMIT=0
    AUTO_PUSH=0
    AUTO_ADVANCE=0
    log_msg "[milestone] Dry-run mode: no commits/pushes or index updates will occur."
    log_msg "[milestone] Stage: ${STAGE}."
fi

if [[ "$CI_GATE_ONLY" -eq 1 ]]; then
    GATE_ARGS=()
    if [[ "$CI_GATE_JSON" -eq 1 ]]; then
        GATE_ARGS+=(--json)
    fi
    if [[ "$CI_GATE_SKIP_CURRENT" -eq 1 ]]; then
        GATE_ARGS+=(--skip-current)
    fi
    if [[ "$CI_GATE_SKIP_NEXT" -eq 1 ]]; then
        GATE_ARGS+=(--skip-next)
    fi
    if [[ "$CI_GATE_RUN" -eq 1 ]]; then
        GATE_ARGS+=(--run)
    fi
    if [[ ${#CI_GATE_ARGS[@]} -gt 0 ]]; then
        GATE_ARGS+=("${CI_GATE_ARGS[@]}")
    fi

    bash tools/test-run-ci-gate.sh "${GATE_ARGS[@]}"
    exit 0
fi


if [[ "$STATUS_ONLY" -eq 1 ]]; then
    STATUS_SCOPE="all"
    if [[ "$NEXT_ID_ONLY" -eq 1 ]]; then
        STATUS_SCOPE="next-id"
    elif [[ "$STATUS_CURRENT" -eq 1 ]]; then
        STATUS_SCOPE="current"
    elif [[ "$STATUS_NEXT" -eq 1 ]]; then
        STATUS_SCOPE="next"
    fi

    if ! STATUS_JSON_TEXT="$(bash tools/stage-status.sh --json)"; then
        STATUS_RC=$?
        STATUS_PAYLOAD="{}"
    else
        STATUS_RC=0
        STATUS_PAYLOAD="$STATUS_JSON_TEXT"
    fi

    if [[ "$STATUS_WITH_CI" -eq 1 ]]; then
        if [[ "$STATUS_JSON" -eq 1 || "$STATUS_RAW" -eq 1 || "$QUIET" -eq 1 ]]; then
            CI_PAYLOAD="$(bash tools/test-run-ci-gate.sh --json)"
            CI_RC=$?
        else
            printf '%s\n' "$STATUS_PAYLOAD"
            set +e
            bash tools/test-run-ci-gate.sh
            CI_RC=$?
            set -e
            CI_PAYLOAD="$(bash tools/test-run-ci-gate.sh --json)"
        fi
    fi

    if [[ "$STATUS_JSON" -eq 1 ]]; then
        if [[ "$STATUS_WITH_CI" -eq 1 ]]; then
            python3 - "$STATUS_PAYLOAD" "$CI_PAYLOAD" "$STATUS_SCOPE" "$STATUS_RC" "$CI_RC" <<'PY'
import json
import sys


def parse(payload):
    try:
        return json.loads(payload)
    except Exception:
        return {}

status_payload = parse(sys.argv[1])
ci_payload = parse(sys.argv[2])
status_scope = sys.argv[3]
status_rc = int(sys.argv[4])
ci_rc = int(sys.argv[5])

print(
    json.dumps(
        {
            "status_scope": status_scope,
            "status": status_payload,
            "ci": ci_payload,
            "ready": bool(ci_payload.get("ci", {}).get("ready", False)),
            "status_rc": status_rc,
            "ci_rc": ci_rc,
            "ok": status_rc == 0 and ci_rc == 0,
        },
        sort_keys=True,
    )
)
PY
        else
            python3 - "$STATUS_PAYLOAD" "$STATUS_SCOPE" <<'PY'
import json
import sys


def parse(payload):
    try:
        return json.loads(payload)
    except Exception:
        return {}

payload = parse(sys.argv[1])
status_scope = sys.argv[2]
stages = payload.get('stages', []) if isinstance(payload, dict) else []
current_stage = payload.get('current_stage') if isinstance(payload, dict) else None

if status_scope == 'next-id':
    next_stage = next((s for s in stages if isinstance(s, dict) and s.get('id', 0) > current_stage and s.get('status') != 'done'), None)
    print(json.dumps({'next_id': None if next_stage is None else next_stage.get('id', 0)}))
    raise SystemExit(0)

if status_scope == 'current':
    current = next((s for s in stages if isinstance(s, dict) and s.get('id') == current_stage), None)
    print(json.dumps({'current_stage': current_stage, 'stage': current}, sort_keys=True))
    raise SystemExit(0)

if status_scope == 'next':
    next_stage = next((s for s in stages if isinstance(s, dict) and s.get('id', 0) > current_stage and s.get('status') != 'done'), None)
    print(json.dumps({'current_stage': current_stage, 'next_stage': next_stage.get('id') if next_stage is not None else None, 'stage': next_stage}, sort_keys=True))
    raise SystemExit(0)

print(json.dumps(payload, sort_keys=True))
PY
        fi
    elif [[ "$STATUS_RAW" -eq 1 ]]; then
        if [[ "$STATUS_WITH_CI" -eq 1 ]]; then
            python3 - "$STATUS_PAYLOAD" "$CI_PAYLOAD" "$STATUS_SCOPE" "$STATUS_RC" "$CI_RC" <<'PY'
import json
import sys


def parse(payload):
    try:
        return json.loads(payload)
    except Exception:
        return {}

status = parse(sys.argv[1])
ci = parse(sys.argv[2])
status_scope = sys.argv[3]
status_rc = int(sys.argv[4])
ci_rc = int(sys.argv[5])
ready = bool(ci.get("ci", {}).get("ready", False))

print(f"[milestone:{status_scope}] status_rc={status_rc} ci_rc={ci_rc} ready={ready} ok={status_rc == 0 and ci_rc == 0}")
print(f"status={json.dumps(status)}")
print(f"ci={json.dumps(ci)}")
PY
        else
            python3 - "$STATUS_PAYLOAD" "$STATUS_SCOPE" <<'PY'
import json
import sys


def parse(payload):
    try:
        return json.loads(payload)
    except Exception:
        return {}

payload = parse(sys.argv[1])
stages = payload.get('stages', []) if isinstance(payload, dict) else []
status_scope = sys.argv[2]
current_stage = payload.get('current_stage') if isinstance(payload, dict) else None
current_entry = next((s for s in stages if isinstance(s, dict) and s.get('id') == current_stage), {}) if isinstance(stages, list) else {}
next_stage = payload.get('stage') if isinstance(payload, dict) else None


def fmt(value):
    return '' if value is None else str(value)

if status_scope == 'next-id':
    if isinstance(stages, list) and isinstance(current_stage, int):
        candidate = next((s for s in stages if isinstance(s, dict) and s.get('id', 0) > current_stage and s.get('status') != 'done'), None)
        next_id = candidate.get('id') if isinstance(candidate, dict) else None
    else:
        next_id = None
    if not next_id:
        print('No pending next stage found.')
    else:
        print(f"Next stage id: {fmt(next_id)}")
    raise SystemExit(0)

if status_scope == 'all' and isinstance(stages, list):
    for stage in stages:
        if not isinstance(stage, dict):
            continue
        marker = '>>> ' if stage.get('id') == current_stage else '    '
        print(f"{marker}{stage.get('id', ''):>3} {str(stage.get('status', 'unknown')):>8}  {stage.get('tests', 0):>6}  {stage.get('path', '')}")

    total = sum(stage.get('tests', 0) for stage in stages if isinstance(stage, dict))
    done_tests = sum(stage.get('tests', 0) for stage in stages if isinstance(stage, dict) and stage.get('status') == 'done')
    done = sum(1 for stage in stages if isinstance(stage, dict) and stage.get('status') == 'done')
    pending = len(stages) - done
    print()
    print(f"Done: {done}/{len(stages)} stages ({done_tests}/{total} tests)")
    print(f"Pending: {pending} stages ({total - done_tests} tests)")
    print(f"Current: Stage {fmt(current_stage)}")
    print(f"Progress: {done_tests * 100 / total:.1f}%" if total else 'Progress: 0.0%')
    raise SystemExit(0)

if status_scope == 'all':
    print('No stage data available.')
    raise SystemExit(0)

print(f"Current stage: {fmt(current_stage)}")
print(f"Path:       {fmt(current_entry.get('path', ''))}")
print(f"Status:     {fmt(current_entry.get('status', 'unknown'))}")
print(f"Tests:      {fmt(current_entry.get('tests', 0))}")
print(f"Failed:     {fmt(current_entry.get('failed', 0))}")

if status_scope == 'next':
    if isinstance(next_stage, dict) and next_stage.get('id') not in (None, 0, '0', ''):
        print()
        print(f"Next stage: {fmt(next_stage.get('id'))}")
        print(f"Path:      {fmt(next_stage.get('path', ''))}")
        print(f"Status:    {fmt(next_stage.get('status', 'unknown'))}")
        print(f"Tests:     {fmt(next_stage.get('tests', 0))}")
    else:
        print("No pending next stage found.")
PY
        fi
    fi

    if [[ "$QUIET" -eq 0 && "$STATUS_JSON" -eq 0 ]]; then
        log_msg "[milestone] Current stage is ${STAGE}."
        log_status "status-only" "displayed status only"
        if [[ "$STATUS_WITH_CI" -eq 0 ]]; then
            show_history
        fi
    fi
    if [[ "$STATUS_WITH_CI" -eq 1 && "$CI_RC" -ne 0 ]]; then
        exit 1
    fi
    exit 0
fi
if [[ "$RUN_TEST_RUN" -eq 1 ]]; then
    if [[ "$RUN_TEST_RUN_PREFLIGHT" -eq 1 ]]; then
        bash tools/test-run-preflight.sh || exit 1
    fi
    log_msg "[milestone] Running test-run check for stage ${STAGE}..."
    if bash tools/test-run-stage.sh "$STAGE"; then
        log_msg "[milestone] Test-run check complete for stage ${STAGE}."
        log_status "test-run" "pass"
    else
        if [[ "$AUTO_RERUN" -eq 1 ]]; then
            if [[ "$RERUN_JSON" -eq 1 ]]; then
                bash tools/milestone-rerun.sh --stage "$STAGE" --json --no-log --quiet --out "$RERUN_JSON_OUT"
            else
                bash tools/milestone-rerun.sh --stage "$STAGE"
            fi
        fi
        log_status "test-run" "fail"
        echo "[milestone] Stage ${STAGE} test-run failed." >&2
        exit 1
    fi

    if [[ "$AUTO_ADVANCE" -eq 1 ]]; then
        bash tools/advance-stage.sh
    fi

    if [[ "$AUTO_COMMIT" -eq 1 ]]; then
        if ! git diff --quiet || ! git diff --cached --quiet; then
            if [[ -z "$COMMIT_MESSAGE" ]]; then
                COMMIT_MESSAGE="chore: stage ${STAGE} test-run milestone"
            fi
            git add -A
            git commit -m "$COMMIT_MESSAGE"
            log_msg "[milestone] Committed test-run milestone for stage ${STAGE}."
            if [[ "$AUTO_PUSH" -eq 1 ]]; then
                git push
                log_msg "[milestone] Pushed test-run milestone commit."
            fi
            log_status "test-run" "committed"
        else
            log_msg "[milestone] No working-tree changes to commit."
            log_status "test-run" "passed-no-changes"
        fi
    fi
    if [[ "$QUIET" -eq 0 ]]; then
        bash tools/stage-status.sh
        show_history
    fi
    exit 0
fi

if bash tools/fix-stage.sh; then
    if [[ "$AUTO_ADVANCE" -eq 1 ]]; then
        bash tools/advance-stage.sh
    else
        log_msg "[milestone] Stage $STAGE passed. Add --advance to auto-update index.json."
    fi

    if [[ "$AUTO_COMMIT" -eq 1 ]]; then
        if ! git diff --quiet || ! git diff --cached --quiet; then
            if [[ -z "$COMMIT_MESSAGE" ]]; then
                COMMIT_MESSAGE="chore: stage $STAGE test-run milestone"
            fi
            git add -A
            git commit -m "$COMMIT_MESSAGE"
            log_msg "[milestone] Committed milestone for stage $STAGE."
            if [[ "$AUTO_PUSH" -eq 1 ]]; then
                git push
                log_msg "[milestone] Pushed milestone commit."
            fi
            log_status "test-run" "committed"
        else
            log_msg "[milestone] No working-tree changes to commit."
            log_status "test-run" "passed-no-changes"
        fi
    fi
    log_status "test-run" "passed"
    if [[ "$QUIET" -eq 0 ]]; then
        bash tools/stage-status.sh
        show_history
    fi

    exit 0
else
    echo "[milestone] Stage $STAGE failed. fix-stage already opened the first failing test." >&2
    if [[ "$AUTO_RERUN" -eq 1 ]]; then
        if [[ "$RERUN_JSON" -eq 1 ]]; then
            bash tools/milestone-rerun.sh --stage "$STAGE" --json --no-log --quiet --out "$RERUN_JSON_OUT"
        else
            bash tools/milestone-rerun.sh --stage "$STAGE"
        fi
    fi
    if [[ "$QUIET" -eq 0 ]]; then
        show_history
    fi
    log_status "test-run" "failed"
    exit 1
fi
