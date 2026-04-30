#!/usr/bin/env bash
# Smoke gate: build-only check + pointer to manual lighting checklist.
# Full hardware verification requires docs/baseline/SMOKE_LIGHTING_CHECKLIST.md
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
echo "== CNTRL smoke_lighting: cargo build (workspace: cntrl-core + cntrl) =="
cargo build
echo ""
echo "Build OK."
echo "Manual steps: see docs/baseline/SMOKE_LIGHTING_CHECKLIST.md"
echo "Behavior reference: docs/baseline/BASELINE_BEHAVIOR.md"
