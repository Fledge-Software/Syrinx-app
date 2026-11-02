#!/usr/bin/env bash
# Runs the Linux native fixtures and full gate outside the restricted agent sandbox.
set -euo pipefail

# Refuse missing desktop connections instead of substituting another backend or runtime.
for connection in DISPLAY DBUS_SESSION_BUS_ADDRESS XDG_RUNTIME_DIR; do
    if [[ -z ${!connection:-} ]]; then
        printf 'Required inherited desktop connection is missing: %s\n' "$connection" >&2
        exit 1
    fi
done

# Keep this worktree's build artifacts and synthetic app data separate from every other run.
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd -- "$repo_root"
run_dir=$(mktemp -d /tmp/syrinx-linux-b4-native.XXXXXX)
mkdir -m 700 -- "$run_dir/data" "$run_dir/cache" "$run_dir/config"
export XDG_DATA_HOME="$run_dir/data"
export XDG_CACHE_HOME="$run_dir/cache"
export XDG_CONFIG_HOME="$run_dir/config"
export CARGO_TARGET_DIR="$repo_root/target"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
git rev-parse HEAD | tee "$run_dir/commit.txt"
printf 'Synthetic native run and logs: %s\n' "$run_dir"

# Stop at the first native failure; pipefail retains Cargo's status through tee.
for fixture in page_close cookie_persistence navigation_history_spike; do
    cargo run -p syrinx-platform --example "$fixture" --profile local --offline --locked \
        2>&1 | tee "$run_dir/$fixture.log"
done

# Leave each visible Syrinx acceptance window in front without typing or clicking into it.
cargo xtask palette-acceptance 2>&1 | tee "$run_dir/palette-acceptance.log"
cargo xtask check 2>&1 | tee "$run_dir/gate.log"
printf 'Native fixtures, palette acceptance and full gate passed. Logs: %s\n' "$run_dir"
