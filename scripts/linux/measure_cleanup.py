"""Bound cleanup to recorded process identities so a failed GUI probe cannot leave a run behind."""

from contextlib import contextmanager
import ctypes
import os
from pathlib import Path
import signal
import time

from measure_metrics import process_ids, stat_fields

SET_CHILD_SUBREAPER = 36
GET_CHILD_SUBREAPER = 37

def status(process):
    try:
        fields = stat_fields(Path(f"/proc/{process}/stat").read_text())
    except FileNotFoundError:
        return None
    return int(fields[19]), fields[0], int(fields[1])


def identity(process):
    current = status(process)
    return None if current is None else current[0]


def child_reaper(option, value):
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int] + [ctypes.c_ulong] * 4
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(option, value, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "Cannot scope Linux measurement child cleanup")


@contextmanager
def orphan_scope():
    # Scope the runner itself, so even detached children are adopted when their root exits.
    previous = ctypes.c_int()
    child_reaper(GET_CHILD_SUBREAPER, ctypes.addressof(previous))
    baseline = process_ids(os.getpid())
    child_reaper(SET_CHILD_SUBREAPER, 1)
    try:
        yield baseline
    finally:
        child_reaper(SET_CHILD_SUBREAPER, previous.value)


def remember_tree(known, baseline=None):
    if baseline is not None:
        for process in process_ids(os.getpid()) - baseline:
            if process not in known:
                known[process] = identity(process)
    for root, started in list(known.items()):
        if started is None or identity(root) != started:
            continue
        for process in process_ids(root):
            if process not in known:
                known[process] = identity(process)


def alive(known):
    result = []
    for process, started in known.items():
        current = status(process)
        if current is not None and current[0] == started and current[1] != "Z":
            result.append(process)
    return result


def signal_owned(known, number, baseline=None):
    remember_tree(known, baseline)
    for process in alive(known):
        try:
            os.kill(process, number)
        except ProcessLookupError:
            # A recorded process can exit between the identity check and signal.
            continue


def reap_adopted(known, root):
    for child, started in known.items():
        current = status(child)
        if child == root or current is None:
            continue
        if current != (started, "Z", os.getpid()):
            continue
        os.waitpid(child, 0)


def wait_owned(process, known, seconds, baseline):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        process.poll()
        remember_tree(known, baseline)
        reap_adopted(known, process.pid)
        if not alive(known):
            process.wait(timeout=1)
            return True
        time.sleep(0.05)
    return False


def finish_owned(process, known, baseline):
    if wait_owned(process, known, 1, baseline):
        return
    for number in [signal.SIGTERM, signal.SIGKILL]:
        signal_owned(known, number, baseline)
        if wait_owned(process, known, 2, baseline):
            return
    raise RuntimeError(f"Owned measurement processes did not stop: {alive(known)}")
