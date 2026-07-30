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
        --ssot)
            RUN_TEST_RUN=1
            # Deprecated alias retained for compatibility; canonical command is --test-run.
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
    if [[ "$STATUS_WITH_CI" -eq 1 ]]; then
        set +e
        STATUS_SCOPE="all"
        if [[ "$STATUS_CURRENT" -eq 1 ]]; then
            STATUS_PAYLOAD="$(bash tools/stage-status.sh --json --current)"
            STATUS_SCOPE="current"
        elif [[ "$STATUS_NEXT" -eq 1 ]]; then
            STATUS_PAYLOAD="$(bash tools/stage-status.sh --json --next)"
            STATUS_SCOPE="next"
        elif [[ "$NEXT_ID_ONLY" -eq 1 ]]; then
            NEXT_ID="$(bash tools/stage-status.sh --next-id)"
            STATUS_PAYLOAD="$(python3 - "$NEXT_ID" <<'PY'
import json
import sys

payload = {"next_id": sys.argv[1].strip()}
print(json.dumps(payload))
PY
)"
            STATUS_SCOPE="next-id"
        else
            STATUS_PAYLOAD="$(bash tools/stage-status.sh --json)"
        fi
        STATUS_RC=$?
        if [[ "$STATUS_JSON" -eq 1 || "$STATUS_RAW" -eq 1 ]]; then
            CI_PAYLOAD="$(bash tools/test-run-ci-gate.sh --json)"
            CI_RC=$?
        elif [[ "$QUIET" -eq 0 ]]; then
            printf '%s\n' "$STATUS_PAYLOAD"
            set +e
            bash tools/test-run-ci-gate.sh
            CI_RC=$?
            set -e
        else
            CI_PAYLOAD="$(bash tools/test-run-ci-gate.sh --json)"
            CI_RC=$?
        fi

        if [[ "$STATUS_JSON" -eq 1 ]]; then
            python3 - "$STATUS_PAYLOAD" "$CI_PAYLOAD" "$STATUS_SCOPE" "$STATUS_RC" "$CI_RC" <<'PY'
import json
import sys

status_payload = sys.argv[1]
ci_payload = sys.argv[2]
status_scope = sys.argv[3]
status_rc = int(sys.argv[4])
ci_rc = int(sys.argv[5])


def parse(payload):
    try:
        return json.loads(payload)
    except Exception:
        return {}

status = parse(status_payload)
ci = parse(ci_payload)
print(
    json.dumps(
        {
            "status_scope": status_scope,
            "status": status,
            "ci": ci,
            "ready": bool(ci.get("ci", {}).get("ready", False)),
            "status_rc": status_rc,
            "ci_rc": ci_rc,
            "ok": status_rc == 0 and ci_rc == 0,
        },
        sort_keys=True,
    )
)
PY
        fi
        if [[ "$STATUS_RAW" -eq 1 ]]; then
            STATUS_SCOPE="$STATUS_SCOPE" \
            STATUS_RC="$STATUS_RC" \
            CI_RC="$CI_RC" \
            python3 - "$STATUS_PAYLOAD" "$CI_PAYLOAD" "$STATUS_SCOPE" "$STATUS_RC" "$CI_RC" <<'PY'
import json
import sys

status_payload = sys.argv[1]
ci_payload = sys.argv[2]
status_scope = sys.argv[3]
status_rc = int(sys.argv[4])
ci_rc = int(sys.argv[5])


def parse(payload):
    try:
        return json.loads(payload)
    except Exception:
        return {}

status = parse(status_payload)
ci = parse(ci_payload)
ready = bool(ci.get("ci", {}).get("ready", False))
print(
    f"[milestone:{status_scope}] status_rc={status_rc} ci_rc={ci_rc} ready={ready} ok={status_rc == 0 and ci_rc == 0}"
)
print(f"status={json.dumps(status)}")
print(f"ci={json.dumps(ci)}")
PY
        fi
        set -e
    else
        if [[ "$QUIET" -eq 0 ]]; then
            if [[ "$STATUS_JSON" -eq 1 && "$STATUS_CURRENT" -eq 1 ]]; then
                bash tools/stage-status.sh --json --current
            elif [[ "$STATUS_JSON" -eq 1 && "$STATUS_NEXT" -eq 1 ]]; then
                bash tools/stage-status.sh --json --next
            elif [[ "$STATUS_JSON" -eq 1 ]]; then
                bash tools/stage-status.sh --json
            elif [[ "$STATUS_CURRENT" -eq 1 ]]; then
                bash tools/stage-status.sh --current
            elif [[ "$STATUS_NEXT" -eq 1 ]]; then
                if [[ "$NEXT_ID_ONLY" -eq 1 ]]; then
                    bash tools/stage-status.sh --next-id
                else
                    bash tools/stage-status.sh --next
                fi
            else
                bash tools/stage-status.sh
            fi
        fi
    fi
    log_msg "[milestone] Current stage is ${STAGE}."
    log_status "status-only" "displayed status only"
    if [[ "$QUIET" -eq 0 && "$STATUS_JSON" -eq 0 && "$STATUS_WITH_CI" -eq 0 ]]; then
        show_history
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
