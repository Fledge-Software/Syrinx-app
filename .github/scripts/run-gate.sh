#!/usr/bin/env bash
# Runs the gate, keeps its log, and on failure names the step that stopped it.
set -uo pipefail
mkdir -p target
log=target/gate.log

cargo xtask check 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
[ "$status" -eq 0 ] && exit 0

# A test binary that dies on a native fault prints a Windows STATUS_ code instead of a test failure.
if grep -q 'STATUS_[A-Z_]*' "$log"; then
  echo "crashed=true" >> "$GITHUB_OUTPUT"
fi

# Put the end of the log in the run summary, so the failure is readable without opening the job.
{
  echo "### The gate failed"
  echo '```'
  tail -n 60 "$log"
  echo '```'
} >> "$GITHUB_STEP_SUMMARY"
echo "::error title=Gate failed::$(grep -m1 -E '^(error|Diff in|test .* FAILED)' "$log" || echo "cargo xtask check exited with $status; see the run summary.")"
exit "$status"
