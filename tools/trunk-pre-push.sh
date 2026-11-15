#!/bin/bash
# Pre-push hook for the trunk: a push to `v2-merged` carries only a commit whose tree has a gate
# pass record (`tools/trunk-gate.sh`, `refs/notes/gate`). Install once per clone:
#   ln -sf ../../tools/trunk-pre-push.sh .git/hooks/pre-push && git fetch origin refs/notes/gate:refs/notes/gate
# Git feeds the hook "<local ref> <local sha> <remote ref> <remote sha>" lines on stdin.
trunk=refs/heads/v2-merged
while read -r local_ref local_sha remote_ref remote_sha; do
  [ "$remote_ref" = "$trunk" ] || continue
  tree=$(git rev-parse "$local_sha^{tree}")
  if ! git notes --ref=gate show "$local_sha" 2>/dev/null | grep -q "^gate pass tree=$tree "; then
    echo "pre-push: $local_sha has no gate pass record for tree $tree; run tools/trunk-gate.sh first" >&2
    exit 1
  fi
done
exit 0
