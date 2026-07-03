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

    def make_fake_nexiq(self, bun_body: str) -> tuple[Path, Path]:
        temp_dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, temp_dir, ignore_errors=True)
        nexiq_dir = temp_dir / "nexiq"
        bin_dir = temp_dir / "bin"
        nexiq_dir.mkdir()
        bin_dir.mkdir()

        bun = bin_dir / "bun"
        bun.write_text(
            "#!/usr/bin/env bash\nset -euo pipefail\n" + bun_body,
            encoding="utf-8",
        )
        bun.chmod(0o755)
        return nexiq_dir, bin_dir

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

    def test_nexiq_contract_dry_run_records_command_and_endpoint_env(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            "--dry-run",
            env={
                "AXON_ENDPOINT": "http://127.0.0.1:18181",
                "AXON_TENANT": "tenant-a",
                "AXON_DATABASE": "db-a",
                "AXON_SCHEMA_HASH": "schema-a",
            },
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "RUN_INTEGRATION=1 bun test tests/contract/axon-contract.spec.ts",
            result.stdout,
        )
        self.assertIn("env AXON_ENDPOINT=http://127.0.0.1:18181", result.stdout)
        self.assertIn("env NEXIQ_AXON_ENDPOINT=http://127.0.0.1:18181", result.stdout)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["classification"], "none")

        command = summary["commands"][0]
        self.assertEqual(command["name"], "nexiq-contract")
        self.assertEqual(command["state"], "planned")
        self.assertEqual(
            command["shell"],
            "RUN_INTEGRATION=1 bun test tests/contract/axon-contract.spec.ts",
        )
        self.assertEqual(command["env"]["AXON_ENDPOINT"], "http://127.0.0.1:18181")
        self.assertEqual(
            command["env"]["NEXIQ_AXON_ENDPOINT"], "http://127.0.0.1:18181"
        )
        self.assertEqual(command["env"]["NEXIQ_AXON_TENANT"], "tenant-a")
        self.assertEqual(command["env"]["NEXIQ_AXON_DATABASE"], "db-a")
        self.assertEqual(command["env"]["NEXIQ_AXON_SCHEMA_HASH"], "schema-a")

    def test_nexiq_e2e_dry_run_records_real_axon_script(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "e2e",
            "--dry-run",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("bun run scripts/run-e2e-real-axon.ts", result.stdout)
        command = summary["commands"][0]
        self.assertEqual(command["name"], "nexiq-e2e")
        self.assertEqual(command["shell"], "bun run scripts/run-e2e-real-axon.ts")

    def test_nexiq_contract_missing_checkout_is_missing_workload(self) -> None:
        missing_nexiq = Path(tempfile.mkdtemp()) / "not-present"
        self.addCleanup(shutil.rmtree, missing_nexiq.parent, ignore_errors=True)

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            env={"NEXIQ_WORKLOAD_PATH": str(missing_nexiq)},
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "missing")
        self.assertEqual(summary["classification"], "missing_workload")
        self.assertEqual(summary["commands"], [])
        self.assertNotEqual(summary["status"], "passed")

    def test_nexiq_contract_skipped_tests_are_contract_gap(self) -> None:
        nexiq_dir, bin_dir = self.make_fake_nexiq(
            """
if [[ "${RUN_INTEGRATION:-}" != "1" ]]; then
  echo "RUN_INTEGRATION missing"
  exit 65
fi
if [[ "$*" != "test tests/contract/axon-contract.spec.ts" ]]; then
  echo "unexpected bun args: $*"
  exit 66
fi
printf '%s\\n' '0 pass' '1 skip' 'Ran 1 tests across 1 files.'
"""
        )

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            env={
                "NEXIQ_WORKLOAD_PATH": str(nexiq_dir),
                "PATH": f"{bin_dir}:{os.environ['PATH']}",
            },
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["classification"], "contract_gap")
        self.assertIn("skipped integration tests", summary["failure"]["message"])
        command = summary["commands"][0]
        self.assertEqual(command["exit_code"], 0)
        self.assertEqual(command["executed_tests"], 0)
        self.assertEqual(command["skipped_tests"], 1)

    def test_nexiq_contract_no_tests_is_contract_gap(self) -> None:
        nexiq_dir, bin_dir = self.make_fake_nexiq(
            """
printf '%s\\n' 'No tests found!'
"""
        )

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            env={
                "NEXIQ_WORKLOAD_PATH": str(nexiq_dir),
                "PATH": f"{bin_dir}:{os.environ['PATH']}",
            },
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["classification"], "contract_gap")
        self.assertIn("no integration tests ran", summary["failure"]["message"])
        command = summary["commands"][0]
        self.assertEqual(command["exit_code"], 0)
        self.assertEqual(command["executed_tests"], 0)

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
