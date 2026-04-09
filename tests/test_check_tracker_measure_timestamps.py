from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "check_tracker_measure_timestamps.py"


class CheckTrackerMeasureTimestampsCliTests(unittest.TestCase):
    def run_validator(
        self, records: list[dict[str, object]], *ids: str
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", suffix=".jsonl", delete=False
        ) as handle:
            temp_path = Path(handle.name)
            for record in records:
                handle.write(json.dumps(record))
                handle.write("\n")

        command = [sys.executable, str(SCRIPT), str(temp_path)]
        for bead_id in ids:
            command.extend(["--id", bead_id])

        try:
            return subprocess.run(command, capture_output=True, text=True, check=False)
        finally:
            temp_path.unlink(missing_ok=True)

    def test_scoped_validation_fails_when_requested_bead_has_no_notes(self) -> None:
        result = self.run_validator(
            [{"id": "hx-missing-notes", "updated_at": "2026-04-09T18:00:00Z"}],
            "hx-missing-notes",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("hx-missing-notes (missing notes)", result.stderr)

    def test_scoped_validation_fails_when_requested_bead_has_no_measure_results(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-missing-measure-results",
                    "updated_at": "2026-04-09T18:00:00Z",
                    "notes": "<context-digest>present</context-digest>",
                }
            ],
            "hx-missing-measure-results",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            (
                "hx-missing-measure-results "
                "(missing <measure-results> timestamp evidence)"
            ),
            result.stderr,
        )

    def test_scoped_validation_succeeds_when_requested_bead_has_measure_results(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-has-measure-results",
                    "updated_at": "2026-04-09T18:00:00Z",
                    "notes": (
                        "<measure-results><timestamp>2026-04-09T17:59:59Z</timestamp>"
                        "</measure-results>"
                    ),
                }
            ],
            "hx-has-measure-results",
        )

        self.assertEqual(result.returncode, 0)
        self.assertIn("all measure timestamps are <= updated_at", result.stdout)

    def test_scoped_validation_fails_when_any_measure_results_block_is_malformed(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-malformed-and-valid",
                    "updated_at": "2026-04-09T18:00:00Z",
                    "notes": (
                        "<measure-results><status>PASS</status></measure-results>"
                        "<measure-results><timestamp>2026-04-09T17:59:59Z</timestamp>"
                        "</measure-results>"
                    ),
                }
            ],
            "hx-malformed-and-valid",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "hx-malformed-and-valid has measure-results block with missing timestamp",
            result.stderr,
        )

    def test_scoped_validation_succeeds_when_all_measure_results_blocks_are_valid(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-two-valid-measure-results",
                    "updated_at": "2026-04-09T18:00:00Z",
                    "notes": (
                        "<measure-results><timestamp>2026-04-09T17:58:59Z</timestamp>"
                        "</measure-results>"
                        "<measure-results><timestamp>2026-04-09T17:59:59Z</timestamp>"
                        "</measure-results>"
                    ),
                }
            ],
            "hx-two-valid-measure-results",
        )

        self.assertEqual(result.returncode, 0)
        self.assertIn("all measure timestamps are <= updated_at", result.stdout)

    def test_scoped_validation_fails_when_measure_results_block_has_multiple_timestamps(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-multiple-timestamps",
                    "updated_at": "2026-04-09T18:00:00Z",
                    "notes": (
                        "<measure-results>"
                        "<timestamp>2026-04-09T17:58:59Z</timestamp>"
                        "<timestamp>2026-04-09T17:59:59Z</timestamp>"
                        "</measure-results>"
                    ),
                }
            ],
            "hx-multiple-timestamps",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "hx-multiple-timestamps has measure-results block with multiple timestamps",
            result.stderr,
        )

    def test_scoped_validation_fails_when_measure_results_block_is_unclosed(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-unclosed-measure-results",
                    "updated_at": "2026-04-09T19:00:00Z",
                    "notes": (
                        "<measure-results>"
                        "<timestamp>2026-04-09T18:59:11Z</timestamp>"
                    ),
                }
            ],
            "hx-unclosed-measure-results",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            (
                "hx-unclosed-measure-results has measure-results block with "
                "missing closing </measure-results>"
            ),
            result.stderr,
        )

    def test_scoped_validation_ignores_quoted_timestamp_text_in_evidence_attribute(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-quoted-timestamp-snippet",
                    "updated_at": "2026-04-09T19:00:00Z",
                    "notes": (
                        "<measure-results>"
                        "<timestamp>2026-04-09T18:59:11Z</timestamp>"
                        "<review-passes>"
                        "<pass name='correctness' status='issue' "
                        "evidence='quoted prior note "
                        "<measure-results><timestamp>2026-04-09T18:53:00Z</timestamp>"
                        "</measure-results>'/>"
                        "</review-passes>"
                        "</measure-results>"
                    ),
                }
            ],
            "hx-quoted-timestamp-snippet",
        )

        self.assertEqual(result.returncode, 0)
        self.assertIn("all measure timestamps are <= updated_at", result.stdout)

    def test_scoped_validation_ignores_fake_measure_results_before_real_block(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-fake-before-real",
                    "updated_at": "2026-04-09T19:00:00Z",
                    "notes": (
                        "<report-summary>"
                        "quoted prior "
                        "<measure-results>"
                        "<timestamp>2026-04-09T18:53:00Z</timestamp>"
                        "</measure-results>"
                        "</report-summary>"
                        "<measure-results>"
                        "<timestamp>2026-04-09T18:59:11Z</timestamp>"
                        "</measure-results>"
                    ),
                }
            ],
            "hx-fake-before-real",
        )

        self.assertEqual(result.returncode, 0)
        self.assertIn("all measure timestamps are <= updated_at", result.stdout)

    def test_scoped_validation_ignores_fake_measure_results_after_real_block(
        self,
    ) -> None:
        result = self.run_validator(
            [
                {
                    "id": "hx-fake-after-real",
                    "updated_at": "2026-04-09T19:00:00Z",
                    "notes": (
                        "<measure-results>"
                        "<timestamp>2026-04-09T18:59:11Z</timestamp>"
                        "</measure-results>"
                        "<report-summary>"
                        "quoted later "
                        "<measure-results>"
                        "<timestamp>2026-04-09T18:53:00Z</timestamp>"
                        "</measure-results>"
                        "</report-summary>"
                    ),
                }
            ],
            "hx-fake-after-real",
        )

        self.assertEqual(result.returncode, 0)
        self.assertIn("all measure timestamps are <= updated_at", result.stdout)


if __name__ == "__main__":
    unittest.main()
