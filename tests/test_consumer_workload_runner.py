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

    def make_fake_consumer_dir(self, name: str) -> Path:
        temp_dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, temp_dir, ignore_errors=True)
        consumer_dir = temp_dir / name
        consumer_dir.mkdir()
        return consumer_dir

    def make_git_checkout(self, name: str, *, dirty: bool) -> tuple[Path, str]:
        temp_dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, temp_dir, ignore_errors=True)
        repo_dir = temp_dir / name
        repo_dir.mkdir()

        def run_git(*args: str) -> None:
            subprocess.run(
                ["git", *args],
                cwd=repo_dir,
                check=True,
                capture_output=True,
                text=True,
            )

        run_git("init", "-q")
        run_git("config", "user.email", "test@example.com")
        run_git("config", "user.name", "Test")
        (repo_dir / "README.md").write_text("init\n", encoding="utf-8")
        run_git("add", "README.md")
        run_git("commit", "-q", "-m", "init")

        sha = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repo_dir,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

        if dirty:
            (repo_dir / "README.md").write_text("dirty\n", encoding="utf-8")

        return repo_dir, sha

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

    def test_cayce_contract_missing_source_is_missing_workload(self) -> None:
        missing_cayce = Path(tempfile.mkdtemp()) / "not-present"
        self.addCleanup(shutil.rmtree, missing_cayce.parent, ignore_errors=True)

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "cayce",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            env={"CAYCE_WORKLOAD_PATH": str(missing_cayce)},
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["consumer"], "cayce")
        self.assertEqual(summary["status"], "missing")
        self.assertEqual(summary["classification"], "missing_workload")
        self.assertEqual(summary["commands"], [])
        self.assertNotEqual(summary["status"], "passed")
        self.assertIn("Cayce workload source", summary["failure"]["message"])

    def test_ddx_contract_dry_run_is_blocked_contract_gap_without_contract(self) -> None:
        missing_ddx = Path(tempfile.mkdtemp()) / "not-present"
        self.addCleanup(shutil.rmtree, missing_ddx.parent, ignore_errors=True)

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "ddx",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            "--dry-run",
            env={"DDX_WORKLOAD_PATH": str(missing_ddx)},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("future proof", result.stdout)
        self.assertIn("classification=contract_gap", result.stdout)
        self.assertEqual(summary["consumer"], "ddx")
        self.assertEqual(summary["status"], "blocked")
        self.assertEqual(summary["classification"], "contract_gap")
        self.assertEqual(summary["exit_code"], 0)
        self.assertNotEqual(summary["status"], "passed")
        self.assertIn("not configured", summary["failure"]["message"])

        command = summary["commands"][0]
        self.assertEqual(command["name"], "ddx-future-real-axon-proof")
        self.assertEqual(command["state"], "planned")
        self.assertEqual(command["env"]["DDX_AXON_ENDPOINT"], "http://127.0.0.1:0")

    def test_ddx_configured_fake_transport_is_contract_gap(self) -> None:
        ddx_dir = self.make_fake_consumer_dir("ddx")

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "ddx",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            env={
                "DDX_WORKLOAD_PATH": str(ddx_dir),
                "DDX_REAL_AXON_WORKLOAD_COMMAND": (
                    "printf '%s\\n' 'fake transport passed' "
                    "'executed_tests=1 skipped_tests=0' 'real_axon_wire_calls=1'"
                ),
            },
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["classification"], "contract_gap")
        self.assertNotEqual(summary["status"], "passed")
        self.assertIn("fake transport", summary["failure"]["message"])
        self.assertEqual(summary["commands"][0]["exit_code"], 0)

    def test_ddx_explicit_real_contract_can_pass_with_wire_evidence(self) -> None:
        ddx_dir = self.make_fake_consumer_dir("ddx")

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "ddx",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            env={
                "DDX_WORKLOAD_PATH": str(ddx_dir),
                "DDX_REAL_AXON_WORKLOAD_COMMAND": (
                    "printf '%s\\n' 'real_axon_wire_calls=1' "
                    "'executed_tests=1 skipped_tests=0'"
                ),
            },
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["classification"], "none")
        self.assertEqual(summary["commands"][0]["name"], "ddx-real-axon-contract")

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

    def test_release_mode_accepts_native_machine_readable_test_counts(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "fake",
            "--backend",
            "sqlite",
            "--mode",
            "release",
            env={
                "AXON_FAKE_WORKLOAD_MARKER": '{"executed_tests":1,"skipped_tests":0}',
            },
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["classification"], "none")
        command = summary["commands"][0]
        self.assertEqual(command["executed_tests"], 1)
        self.assertEqual(command["skipped_tests"], 0)
        self.assertEqual(command["test_counts_source"], "native")

    def test_release_mode_rejects_marker_only_test_counts(self) -> None:
        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "fake",
            "--backend",
            "sqlite",
            "--mode",
            "release",
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["classification"], "contract_gap")
        self.assertIn("native machine-readable test counts", summary["failure"]["message"])
        command = summary["commands"][0]
        self.assertEqual(command["executed_tests"], 1)
        self.assertEqual(command["skipped_tests"], 0)
        self.assertEqual(command["test_counts_source"], "heuristic")

    def test_self_test_records_null_consumer_sha_and_clean_dirty_state(self) -> None:
        _result, summary, _run_dir = self.run_runner("--self-test")

        self.assertIsNone(summary["consumer_path"])
        self.assertIsNone(summary["consumer_sha"])
        self.assertFalse(summary["consumer_dirty"])

    def test_dry_run_records_consumer_sha_and_dirty_state_for_git_checkout(self) -> None:
        nexiq_dir, expected_sha = self.make_git_checkout("nexiq", dirty=False)

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            "--dry-run",
            env={"NEXIQ_WORKLOAD_PATH": str(nexiq_dir)},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(summary["consumer_path"], str(nexiq_dir))
        self.assertEqual(summary["consumer_sha"], expected_sha)
        self.assertFalse(summary["consumer_dirty"])

    def test_release_mode_fails_on_dirty_consumer_checkout(self) -> None:
        nexiq_dir, expected_sha = self.make_git_checkout("nexiq", dirty=True)

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "release",
            env={"NEXIQ_WORKLOAD_PATH": str(nexiq_dir)},
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["classification"], "consumer_dirty")
        self.assertEqual(summary["consumer_path"], str(nexiq_dir))
        self.assertEqual(summary["consumer_sha"], expected_sha)
        self.assertTrue(summary["consumer_dirty"])
        self.assertIn("dirty", summary["failure"]["message"].lower())
        # The dirty gate runs before any workload command, so no command
        # (e.g. a real bun invocation) is ever attempted.
        self.assertEqual(summary["commands"], [])

    def test_pr_mode_records_but_does_not_gate_on_dirty_consumer_checkout(self) -> None:
        nexiq_dir, expected_sha = self.make_git_checkout("nexiq", dirty=True)

        result, summary, _run_dir = self.run_runner(
            "--consumer",
            "nexiq",
            "--backend",
            "sqlite",
            "--mode",
            "contract",
            "--dry-run",
            env={"NEXIQ_WORKLOAD_PATH": str(nexiq_dir)},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(summary["consumer_sha"], expected_sha)
        self.assertTrue(summary["consumer_dirty"])
        self.assertEqual(summary["status"], "passed")

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
