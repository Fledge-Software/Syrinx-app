"""Protect benchmark accounting from process-name parsing and missing-memory mistakes."""

import unittest

from measure_metrics import cpu_delta, memory_summary, parse_stat


class MeasurementAccounting(unittest.TestCase):
    def test_spaces_and_parentheses_in_a_process_name_do_not_shift_fields(self):
        fields = ["S"] + ["0"] * 21
        fields[11], fields[12], fields[19], fields[21] = "100", "50", "1234", "7"
        record = parse_stat("42 (synthetic) worker) " + " ".join(fields), 4096)
        self.assertEqual(record, {"cpu_ticks": 150, "start_ticks": 1234, "rss_kib": 28})

    def test_two_busy_cores_can_exceed_one_hundred_percent(self):
        before = {1: {"cpu_ticks": 100, "start_ticks": 1}, 2: {"cpu_ticks": 50, "start_ticks": 2}}
        after = {1: {"cpu_ticks": 300, "start_ticks": 1}, 2: {"cpu_ticks": 150, "start_ticks": 2}}
        self.assertEqual(cpu_delta(before, after, 2, 100), {"percent": 150.0, "reason": None})

    def test_process_exit_birth_or_pid_reuse_invalidates_cpu_accounting(self):
        before = {1: {"cpu_ticks": 100, "start_ticks": 1}}
        after = {1: {"cpu_ticks": 300, "start_ticks": 2}}
        for changed in ({}, after, before | {2: {"cpu_ticks": 0, "start_ticks": 4}}):
            self.assertIsNone(cpu_delta(before, changed, 2, 100)["percent"])

    def test_missing_pss_is_unavailable_instead_of_zero(self):
        records = [{"rss_kib": 100, "pss_kib": 50}, {"rss_kib": 200, "pss_kib": None}]
        self.assertEqual(memory_summary(records), {"rss_sum_kib": 300, "pss_sum_kib": None, "pss_unreadable_processes": 1})


if __name__ == "__main__":
    unittest.main()
