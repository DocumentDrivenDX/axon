from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "run-consumer-workloads.sh"


class ConsumerWorkloadRunnerTests(unittest.TestCase):
    def run_runner(
        self,
        *args: str,
        env: dict[str, str] | None = None,
    ) -> tuple[subprocess.CompletedProcess[str], dict[str, object], Path]:
        temp_dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, temp_dir, ignore_errors=True)
        run_dir = temp_dir / "run"
        command = [str(SCRIPT), *args, "--run-dir", str(run_dir)]
        child_env = os.environ.copy()
        if env:
            child_env.update(env)

        result = subprocess.run(
            command,
            cwd=REPO_ROOT,
            env=child_env,
            capture_output=True,
            text=True,
            check=False,
        )

        summary_path = run_dir / "summary.json"
        self.assertTrue(
            summary_path.exists(),
            f"summary missing; stdout={result.stdout!r} stderr={result.stderr!r}",
        )
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        return result, summary, run_dir

    def test_bash_syntax_is_valid(self) -> None:
        result = subprocess.run(
            ["bash", "-n", str(SCRIPT)],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_self_test_writes_required_summary_and_command_logs(self) -> None:
        result, summary, _run_dir = self.run_runner("--self-test")

        self.assertEqual(result.returncode, 0, result.stderr)
        for key in (
            "consumer",
            "backend",
            "status",
            "classification",
            "commands",
            "axon_sha",
        ):
            self.assertIn(key, summary)

        self.assertEqual(summary["consumer"], "fake")
        self.assertEqual(summary["backend"], "sqlite")
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["classification"], "none")
        self.assertIsInstance(summary["commands"], list)
        self.assertGreater(len(summary["commands"]), 0)
        self.assertIsInstance(summary["axon_sha"], str)
        self.assertTrue(summary["axon_sha"])

        command = summary["commands"][0]
        self.assertEqual(command["exit_code"], 0)
        self.assertEqual(command["executed_tests"], 1)
        self.assertEqual(command["skipped_tests"], 0)
        self.assertTrue(Path(command["stdout"]).exists())
        self.assertTrue(Path(command["stderr"]).exists())

    def test_dry_run_records_planned_command_without_executing_it(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "fake",
            "--backend",
            "sqlite",
            "--mode",
            "pr",
            "--dry-run",
            env={"AXON_FAKE_WORKLOAD_EXIT": "91"},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("DRY RUN", result.stdout)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["classification"], "none")
        self.assertEqual(summary["exit_code"], 0)

        command = summary["commands"][0]
        self.assertEqual(command["state"], "planned")
        self.assertIsNone(command["exit_code"])
        self.assertEqual(command["env"]["AXON_ENDPOINT"], "http://127.0.0.1:0")

    def test_fake_failure_is_classified_from_command_evidence(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "fake",
            "--backend",
            "sqlite",
            env={
                "AXON_FAKE_WORKLOAD_EXIT": "9",
                "AXON_FAKE_WORKLOAD_MARKER": "CONTRACT_GAP",
            },
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["classification"], "contract_gap")
        self.assertEqual(summary["exit_code"], 1)
        self.assertEqual(summary["commands"][0]["exit_code"], 9)
        self.assertIn("command 'fake-consumer-contract' failed", str(summary["failure"]))

    def test_release_mode_fails_missing_consumer_workload(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "not-present",
            "--mode",
            "release",
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "missing")
        self.assertEqual(summary["classification"], "missing_workload")


if __name__ == "__main__":
    unittest.main()
