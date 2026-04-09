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


if __name__ == "__main__":
    unittest.main()
