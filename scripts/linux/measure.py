"""Measure isolated synthetic Linux browsing without touching the owner's browser profiles."""

import argparse
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

from measure_metrics import cpu_delta, memory_summary, snapshot
from measure_cleanup import finish_owned, identity, orphan_scope, remember_tree, signal_owned


class SyntheticPages(BaseHTTPRequestHandler):
    def log_message(self, *_arguments):
        return

    def do_GET(self):
        key = self.path.removeprefix("/")
        if key.startswith("ready/"):
            with self.server.lock:
                self.server.ready[key.removeprefix("ready/")] = time.monotonic()
            self.send_response(204)
            self.end_headers()
            return
        if not key.startswith(("startup-", "syrinx-", "reference-")) or not all(
            character.isalnum() or character == "-" for character in key
        ):
            self.send_error(404)
            return
        paragraphs = "<p>Synthetic amber document text for memory comparison.</p>" * 256
        body = (
            "<!doctype html><title>Synthetic measurement tab</title>"
            "<style>body{font:16px sans-serif;margin:32px}</style>"
            f"<h1>Synthetic document</h1>{paragraphs}"
            f"<script>fetch('/ready/{key}');</script>"
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def probe(process, action, window=None):
    command = [sys.executable, str(Path(__file__).with_name("measure_x11.py")), str(process), action]
    if window is not None:
        command.append(str(window))
    output = subprocess.check_output(command, text=True, stderr=subprocess.PIPE, timeout=5)
    return json.loads(output) if output.strip() else None


def wait_for(predicate, process, label, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Measurement process exited before {label}: {process.returncode}")
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise RuntimeError(f"Timed out waiting for {label}; inspect the scratch app log")


def main_window(process):
    windows = probe(process.pid, "list")["windows"]
    for window in windows:
        if window["title"] != "Syrinx · integrated preview":
            continue
        if probe(process.pid, "state", window["id"])["mapped"]:
            return window["id"]
    return None


@contextmanager
def application(command, environment, log, native_close=True):
    with orphan_scope() as baseline, log.open("w") as output:
        process = subprocess.Popen(command, env=environment, stdout=output, stderr=output, start_new_session=True)
        known = {process.pid: identity(process.pid)}
        try:
            yield process
        finally:
            remember_tree(known, baseline)
            try:
                if process.poll() is None:
                    if native_close:
                        window = main_window(process)
                        if window is None:
                            raise RuntimeError("No main window available for orderly measurement shutdown")
                        probe(process.pid, "close", window)
                    else:
                        signal_owned(known, signal.SIGTERM, baseline)
                    process.wait(timeout=10)
            finally:
                # Preserve probe/wait failures while still stopping every recorded descendant.
                finish_owned(process, known, baseline)


def isolated_environment(scratch):
    environment = os.environ.copy()
    # Unix socket names include this prefix and must fit sockaddr_un.sun_path.
    environment["XDG_RUNTIME_DIR"] = tempfile.mkdtemp(prefix="sx-runtime-", dir="/tmp")
    for name, leaf in [("XDG_DATA_HOME", "data"),
                       ("XDG_CACHE_HOME", "cache"), ("XDG_CONFIG_HOME", "config")]:
        folder = scratch / leaf
        folder.mkdir(mode=0o700)
        environment[name] = str(folder)
    return environment


def page_ready(server, key):
    with server.lock:
        return server.ready.get(key)


def focus_state(process, window):
    state = probe(process.pid, "state", window)
    if os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        # Discard every field except pid; X11 focus alone can be stale under Wayland.
        active = subprocess.check_output(["hyprctl", "activewindow", "-j"], text=True, stderr=subprocess.PIPE)
        state["focused"] = json.loads(active).get("pid") == process.pid
    return state


def matches_state(mode, state):
    if mode == "minimized":
        return state["hidden"]
    return state["mapped"] and not state["hidden"] and state["focused"] == (mode == "focused")


def idle_sample(process, window, mode, seconds):
    before_state = focus_state(process, window)
    before = snapshot(process.pid)
    started = time.monotonic()
    time.sleep(seconds)
    after = snapshot(process.pid)
    elapsed = time.monotonic() - started
    after_state = focus_state(process, window)
    result = cpu_delta(before, after, elapsed, os.sysconf("SC_CLK_TCK"))
    if mode != "software-validation" and not (matches_state(mode, before_state) and matches_state(mode, after_state)):
        result = {"percent": None, "reason": "requested window state was not present at both endpoints"}
    return result | {"elapsed_seconds": elapsed, "processes": len(after), "memory": memory_summary(list(after.values()))}


def idle_measurements(process, window, options):
    if options.software_validation:
        return {"software-validation": idle_sample(process, window, "software-validation", options.seconds)}
    result = {}
    for mode in ["focused", "unfocused", "minimized"]:
        choice = input(f"{mode}: Enter starts a 5-second preparation countdown; type 'skip' if unsupported: ")
        if choice == "skip":
            result[mode] = {"percent": None, "reason": "operator reports this window state is unsupported"}
            continue
        print(f"Set the scratch Syrinx window {mode} now; keep that state during the sample.", flush=True)
        time.sleep(5)
        result[mode] = idle_sample(process, window, mode, options.seconds)
        print(json.dumps({mode: result[mode]}), flush=True)
    return result


def measure_startups(binary, scratch, environment, server, base, count):
    results = []
    for number in range(count):
        key = f"startup-{number}"
        command = [str(binary), "--test-data-dir", str(scratch / key), "--", base + "/" + key]
        started = time.monotonic()
        with application(command, environment, scratch / f"{key}.log") as process:
            wait_for(lambda: main_window(process), process, "mapped main window")
            mapped = time.monotonic() - started
            ready = wait_for(lambda: page_ready(server, key), process, "synthetic document readiness")
            results.append({"mapped_main_seconds": mapped, "document_ready_seconds": ready - started})
    return results


def measure_syrinx(binary, scratch, environment, server, base, options):
    profile = scratch / "syrinx-profile"
    command = [str(binary), "--test-data-dir", str(profile)]
    with application(command, environment, scratch / "syrinx.log") as process:
        window = wait_for(lambda: main_window(process), process, "mapped main window")
        time.sleep(options.settle)
        idle = idle_measurements(process, window, options)
        for number in range(10):
            subprocess.run(command + ["--", f"{base}/syrinx-{number}"], env=environment,
                           stdout=subprocess.DEVNULL, check=True, timeout=30)
        wait_for(lambda: all(page_ready(server, f"syrinx-{number}") for number in range(10)), process, "ten loaded Syrinx documents")
        time.sleep(options.settle)
        memory = memory_summary(list(snapshot(process.pid).values()))
        return {"idle": idle, "ten_tabs": memory}


def measure_reference(scratch, server, base, options):
    if options.reference == "none":
        return {"status": "not requested"}
    browser = shutil.which(options.reference)
    if browser is None:
        return {"status": "not installed", "browser": options.reference}
    reference = scratch / "reference"
    reference.mkdir()
    environment = isolated_environment(reference)
    urls = [f"{base}/reference-{number}" for number in range(10)]
    if options.reference == "firefox":
        profile = reference / "profile"
        profile.mkdir()
        command = [browser, "--no-remote", "--profile", str(profile)] + urls
    else:
        command = [browser, f"--user-data-dir={reference / 'profile'}", "--no-first-run",
                   "--no-default-browser-check", "--disable-background-networking", "--disable-sync",
                   "--disable-component-update", "--disable-default-apps"] + urls
    with application(command, environment, reference / "browser.log", native_close=False) as process:
        wait_for(lambda: all(page_ready(server, f"reference-{number}") for number in range(10)), process, "ten reference documents")
        time.sleep(options.settle)
        return {"status": "measured", "browser": options.reference,
                "ten_tabs": memory_summary(list(snapshot(process.pid).values()))}


def run(options):
    if not Path("/dev/dri").exists() and not options.software_validation:
        raise RuntimeError("No /dev/dri: use --software-validation for script checks; the coordinator must collect real figures outside this sandbox")
    binary = options.binary.resolve()
    if not binary.is_file():
        raise RuntimeError(f"Build Syrinx first; measurement binary is missing: {binary}")
    options.output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="run-", dir=options.output.resolve()))
    environment = isolated_environment(scratch)
    server = ThreadingHTTPServer(("127.0.0.1", 0), SyntheticPages)
    server.ready, server.lock = {}, threading.Lock()
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    base = f"http://127.0.0.1:{server.server_port}"
    report = {"scope": "software-rendered script validation; not machine acceptance" if options.software_validation else "coordinator hardware run; verify renderer separately",
              "completed": False,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "cpu_definition": "process-tree user+system CPU; one full core is 100 percent",
              "startup_definition": "warm file cache, fresh profiles; mapped X11 main window and authored document ready fetch",
              "seconds": options.seconds, "settle_seconds": options.settle}
    try:
        report["startup"] = measure_startups(binary, scratch, environment, server, base, options.startup_runs)
        report["syrinx"] = measure_syrinx(binary, scratch, environment, server, base, options)
        report["reference"] = measure_reference(scratch, server, base, options)
        report["completed"] = True
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)
        (scratch / "results.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"Synthetic measurement artifacts: {scratch}", flush=True)
    print(json.dumps(report, indent=2))


def arguments():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=root / "target/local/syrinx")
    parser.add_argument("--output", type=Path, default=root / "target/linux-measurements")
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--settle", type=float, default=5)
    parser.add_argument("--startup-runs", type=int, default=3)
    parser.add_argument("--reference", choices=["chromium", "firefox", "none"], default="chromium")
    parser.add_argument("--software-validation", action="store_true")
    options = parser.parse_args()
    if options.seconds <= 0 or options.settle < 0 or options.startup_runs < 1:
        parser.error("interval and startup count must be positive; settling time cannot be negative")
    return options


if __name__ == "__main__":
    run(arguments())
