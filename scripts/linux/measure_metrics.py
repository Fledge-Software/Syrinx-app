"""Define process-tree CPU and memory accounting for the Linux measurement runner."""

import os
from pathlib import Path
import time


def stat_fields(text):
    return text.rsplit(") ", 1)[1].split()


def parse_stat(text, page_bytes):
    fields = stat_fields(text)
    return {
        "cpu_ticks": int(fields[11]) + int(fields[12]),
        "start_ticks": int(fields[19]),
        "rss_kib": int(fields[21]) * page_bytes // 1024,
    }


def cpu_delta(before, after, elapsed, ticks_per_second):
    if elapsed <= 0 or ticks_per_second <= 0:
        raise ValueError("CPU accounting needs a positive interval and clock frequency")
    if before.keys() != after.keys():
        return {"percent": None, "reason": "process tree changed during the interval"}
    for process, initial in before.items():
        final = after[process]
        if initial["start_ticks"] != final["start_ticks"]:
            return {"percent": None, "reason": "a process identity changed during the interval"}
        if final["cpu_ticks"] < initial["cpu_ticks"]:
            return {"percent": None, "reason": "a CPU counter moved backwards"}
    used = sum(after[process]["cpu_ticks"] - initial["cpu_ticks"] for process, initial in before.items())
    return {"percent": 100 * used / ticks_per_second / elapsed, "reason": None}


def memory_summary(records):
    missing = sum(record["pss_kib"] is None for record in records)
    return {
        "rss_sum_kib": sum(record["rss_kib"] for record in records),
        "pss_sum_kib": None if missing else sum(record["pss_kib"] for record in records),
        "pss_unreadable_processes": missing,
    }


def process_ids(root):
    """Include children spawned by every thread, not just the process leader."""
    found, pending = set(), [root]
    while pending:
        process = pending.pop()
        if process in found:
            continue
        found.add(process)
        for children in Path(f"/proc/{process}/task").glob("*/children"):
            try:
                pending.extend(int(child) for child in children.read_text().split())
            except FileNotFoundError:
                # A thread can finish between globbing its entry and reading it.
                continue
    return found


def read_process(process):
    """Keep denied proportional-memory reads unavailable rather than inventing a zero."""
    root = Path(f"/proc/{process}")
    record = parse_stat((root / "stat").read_text(), os.sysconf("SC_PAGE_SIZE"))
    record["pss_kib"] = None
    try:
        rollup = (root / "smaps_rollup").read_text()
    except PermissionError:
        record["pss_reason"] = "kernel denied smaps_rollup"
        return record
    for line in rollup.splitlines():
        if line.startswith("Pss:"):
            record["pss_kib"] = int(line.split()[1])
            return record
    record["pss_reason"] = "kernel did not provide Pss"
    return record


def snapshot(root):
    """Retry a disappearing child briefly, then report unstable accounting explicitly."""
    for attempt in range(3):
        try:
            return {process: read_process(process) for process in process_ids(root)}
        except FileNotFoundError:
            if attempt == 2:
                raise RuntimeError("Process tree changed repeatedly while reading memory") from None
            time.sleep(0.05)
