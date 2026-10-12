#!/bin/sh
export QUENCH_GC_MARK_THREADS=1
exec /workspace/quench/work/task94-v2-cloud-slotgen-free-bitmap-candidate "$@"
