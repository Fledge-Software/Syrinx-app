"""Prove state classification and cleanup using synthetic subprocesses, never a real profile."""

import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from measure import application, matches_state


def running(process):
    try:
        return Path(f"/proc/{process}/stat").read_text().rsplit(") ", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


def kill_fixture(process):
    if not running(process):
        return
    try:
        os.kill(process, signal.SIGKILL)
    except ProcessLookupError:
        # Exit between the identity check and signal already satisfies cleanup.
        return


class MeasurementLifecycle(unittest.TestCase):
    def test_unfocused_requires_a_visible_non_minimized_window(self):
        self.assertTrue(matches_state("unfocused", {"focused": False, "mapped": True, "hidden": False}))
        self.assertFalse(matches_state("unfocused", {"focused": False, "mapped": False, "hidden": True}))
        self.assertFalse(matches_state("focused", {"focused": True, "mapped": False, "hidden": True}))

    def test_failed_close_probe_still_reaps_the_owned_root(self):
        process = None
        with tempfile.TemporaryDirectory(prefix="sx-cleanup-test-") as folder:
            try:
                with patch("measure.main_window", side_effect=RuntimeError("synthetic probe failure")):
                    with self.assertRaisesRegex(RuntimeError, "synthetic probe failure"):
                        with application([sys.executable, "-c", "import time; time.sleep(100)"],
                                         os.environ.copy(), Path(folder) / "log") as process:
                            time.sleep(0.1)
                self.assertIsNotNone(process.poll(), "failed close left the measured root running")
            finally:
                if process is not None:
                    kill_fixture(process.pid)
                    process.wait(timeout=5)

    def test_reference_cleanup_stops_a_descendant_that_ignores_termination(self):
        process, child = None, None
        with tempfile.TemporaryDirectory(prefix="sx-cleanup-test-") as folder:
            ready = Path(folder) / "ready"
            child_code = "import os,signal,sys,time; from pathlib import Path; signal.signal(signal.SIGTERM,signal.SIG_IGN); Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(100)"
            parent_code = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{child_code!r},sys.argv[1]]); time.sleep(100)"
            try:
                with application([sys.executable, "-c", parent_code, str(ready)], os.environ.copy(),
                                 Path(folder) / "log", native_close=False) as process:
                    deadline = time.monotonic() + 5
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists(), "synthetic descendant never became ready")
                    child = int(ready.read_text())
                self.assertFalse(running(child), "reference cleanup left a measured descendant running")
            finally:
                if child is not None:
                    kill_fixture(child)
                if process is not None:
                    kill_fixture(process.pid)
                    process.wait(timeout=5)

    def test_cleanup_stops_a_child_after_its_parent_exits_first(self):
        process, child = None, None
        with tempfile.TemporaryDirectory(prefix="sx-cleanup-test-") as folder:
            ready = Path(folder) / "ready"
            child_code = "import os,sys,time; from pathlib import Path; Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(100)"
            parent_code = f"import subprocess,sys; subprocess.Popen([sys.executable,'-c',{child_code!r},sys.argv[1]],start_new_session=True)"
            try:
                with application([sys.executable, "-c", parent_code, str(ready)], os.environ.copy(),
                                 Path(folder) / "log", native_close=False) as process:
                    deadline = time.monotonic() + 5
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists())
                    child = int(ready.read_text())
                    process.wait(timeout=5)
                self.assertFalse(running(child), "an already-orphaned descendant escaped cleanup")
            finally:
                if child is not None:
                    kill_fixture(child)
                if process is not None:
                    kill_fixture(process.pid)
                    process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
