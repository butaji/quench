#!/usr/bin/env bash
# Milestone helper: run the stage failure triage flow and optionally advance index.json.
# Usage:
#   bash tools/milestone.sh                    # uses TEST262_STAGE if set, else current_stage
#   bash tools/milestone.sh --stage 32 --advance
#   bash tools/milestone.sh --stage 32 --commit
#   bash tools/milestone.sh --stage 32 --ssot           # run ssot-stage first
#   bash tools/milestone.sh --commit --push  # uses TEST262_STAGE if set, else current_stage
#   bash tools/milestone.sh --status                   # print stage progress and current stage
#   bash tools/milestone.sh --stage 32 --ssot --commit --push

set -euo pipefail

cd "$(dirname "$0")/.."

STAGE=""
STATUS_ONLY=0
RUN_SSOT=0
AUTO_ADVANCE=0
AUTO_COMMIT=0
AUTO_PUSH=0
COMMIT_MESSAGE=""

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
        --status)
            STATUS_ONLY=1
            shift
            ;;
        --ssot)
            RUN_SSOT=1
            shift
            ;;
        --advance)
            AUTO_ADVANCE=1
            shift
            ;;
        -h|--help)
            sed -n '1,140p' "$0"
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

STAGE="${STAGE:-${TEST262_STAGE:-$(python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])")}}"
export TEST262_STAGE="$STAGE"
export TEST262_QUICK=1

log_status() {
    local outcome="$1"
    local note="$2"
    local log_file="${MILESTONE_LOG:-./.test262_milestones.log}"
    {
        printf "[%s] stage=%s outcome=%s " \
            "$(date +'%Y-%m-%d %H:%M:%S%z')" \
            "$STAGE" \
            "$outcome"
        printf "branch=%s " "$(git branch --show-current)"
        printf "commit=%s " "$(git rev-parse --short HEAD)"
        printf "note=%s\n" "$note"
    } >> "$log_file"
}

if [[ "$STATUS_ONLY" -eq 1 ]]; then
    bash tools/stage-status.sh
    echo "[milestone] Current stage is ${STAGE}."
    log_status "status-only" "displayed status only"
    exit 0
fi

if [[ "$RUN_SSOT" -eq 1 ]]; then
    echo "[milestone] Running SSOT check for stage ${STAGE}..."
    if bash tools/ssot-stage.sh "$STAGE"; then
        echo "[milestone] SSOT check complete for stage ${STAGE}."
        log_status "ssot" "pass"
    else
        log_status "ssot" "fail"
        echo "[milestone] Stage ${STAGE} failed SSOT." >&2
        exit 1
    fi

    if [[ "$AUTO_ADVANCE" -eq 1 ]]; then
        bash tools/advance-stage.sh
    fi

    if [[ "$AUTO_COMMIT" -eq 1 ]]; then
        if ! git diff --quiet || ! git diff --cached --quiet; then
            if [[ -z "$COMMIT_MESSAGE" ]]; then
                COMMIT_MESSAGE="chore: stage ${STAGE} ssot milestone"
            fi
            git add -A
            git commit -m "$COMMIT_MESSAGE"
            echo "[milestone] Committed SSOT milestone for stage ${STAGE}."
            if [[ "$AUTO_PUSH" -eq 1 ]]; then
                git push
                echo "[milestone] Pushed SSOT milestone commit."
            fi
            log_status "ssot" "committed"
        else
            echo "[milestone] No working-tree changes to commit."
            log_status "ssot" "passed-no-changes"
        fi
    fi
    bash tools/stage-status.sh
    exit 0
fi

if bash tools/fix-stage.sh; then
    if [[ "$AUTO_ADVANCE" -eq 1 ]]; then
        bash tools/advance-stage.sh
    else
        echo "[milestone] Stage $STAGE passed. Add --advance to auto-update index.json."
    fi

    if [[ "$AUTO_COMMIT" -eq 1 ]]; then
        if ! git diff --quiet || ! git diff --cached --quiet; then
            if [[ -z "$COMMIT_MESSAGE" ]]; then
                COMMIT_MESSAGE="chore: stage $STAGE test-run milestone"
            fi
            git add -A
            git commit -m "$COMMIT_MESSAGE"
            echo "[milestone] Committed milestone for stage $STAGE."
            if [[ "$AUTO_PUSH" -eq 1 ]]; then
                git push
                echo "[milestone] Pushed milestone commit."
            fi
            log_status "test-run" "committed"
        else
            echo "[milestone] No working-tree changes to commit."
            log_status "test-run" "passed-no-changes"
        fi
    fi
    log_status "test-run" "passed"
    bash tools/stage-status.sh

    exit 0
else
    echo "[milestone] Stage $STAGE failed. fix-stage already opened the first failing test." >&2
    log_status "test-run" "failed"
    exit 1
fi
