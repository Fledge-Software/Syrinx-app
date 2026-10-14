#!/usr/bin/env bash
# Runs the test that crashed the one-at-a-time rerun under cdb and prints its native call stack.
set -euo pipefail
log=$1

# The last test line printed before the crash is the crashing test.
name=$(grep -o '^test [^ ]* \.\.\. ' "$log" | tail -n 1 | cut -d' ' -f2)
if [ -z "$name" ]; then
  echo "::warning::No test name found in $log."
  exit 0
fi

exe=$(ls -t target/local/deps/syrinx-*.exe | head -n 1)
cdb="/c/Program Files (x86)/Windows Kits/10/Debuggers/x64/cdb.exe"
if [ ! -x "$cdb" ]; then
  echo "::warning::cdb is not installed on this runner, so no native stack for $name."
  exit 0
fi

# Stop at the first access violation, print the faulting context and stack, then quit.
echo "Native stack for $name in $exe:"
"$cdb" -g -G -lines -c ".ecxr; kn 60; q" "$exe" "$name" --exact --test-threads=1
