#!/usr/bin/env python3
"""Self-tests for check_covers_traceability.py."""

from __future__ import annotations

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import tempfile

from check_covers_traceability import (
    collect_traceability,
    main,
    parse_planned_ac_ids,
    render_json,
    render_text,
)


SCAN_ROOTS = [Path("crates"), Path("ui"), Path("sdk/typescript")]
TEST_PLAN_DIR = Path("docs/helix/03-test/test-plans")


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def test_valid_citations_and_subsystem_counts() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write(
            root / "docs/helix/03-test/test-plans/STP-101-example.md",
            """| AC ID | Criterion |
|-------|-----------|
| US-101-AC1 | one |
| US-101-AC2 | two |
""",
        )
        write(
            root / "crates/axon-server/tests/policy.rs",
            "// @covers US-101-AC1\n",
        )
        write(
            root / "ui/tests/e2e/policy.spec.ts",
            "test('@covers US-101-AC2 renders policy state', () => {});\n",
        )
        write(
            root / "sdk/typescript/test/client.test.ts",
            "it('@covers US-101-AC2 maps policy errors', () => {});\n",
        )

        report = collect_traceability(
            root=root,
            scan_roots=SCAN_ROOTS,
            test_plan_dir=TEST_PLAN_DIR,
        )

        assert report.citations_by_ac == {"US-101-AC1": 1, "US-101-AC2": 2}
        assert report.citations_by_subsystem == {
            "crates/axon-server": 1,
            "sdk/typescript": 1,
            "ui": 1,
        }
        assert report.planned_ac_ids == ["US-101-AC1", "US-101-AC2"]
        assert report.malformed_citations == []


def test_malformed_covers_tokens() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write(root / "crates/axon-api/src/lib.rs", "// @covers US-101\n")
        write(root / "ui/src/lib/api.test.ts", "// @coversUS-101-AC1\n")

        report = collect_traceability(
            root=root,
            scan_roots=SCAN_ROOTS,
            test_plan_dir=TEST_PLAN_DIR,
        )

        assert len(report.malformed_citations) == 2
        assert report.citations_by_ac == {}


def test_stp_parsing_ignores_non_ac_table_cells() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write(
            root / "docs/helix/03-test/test-plans/STP-070-links.md",
            """| AC ID | Citation |
|-------|----------|
| US-070-AC2 | planned `@covers US-070-AC2` |
| not-an-ac | @covers US-070-AC3 |
""",
        )

        assert parse_planned_ac_ids(root, TEST_PLAN_DIR) == ["US-070-AC2"]


def test_renderers_and_cli_exit_codes() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write(
            root / "docs/helix/03-test/test-plans/STP-023-graph.md",
            "| US-023-AC1 | graph |\n",
        )
        write(root / "crates/axon-core/src/lib.rs", "// @covers US-023-AC1\n")

        report = collect_traceability(
            root=root,
            scan_roots=SCAN_ROOTS,
            test_plan_dir=TEST_PLAN_DIR,
        )
        text = render_text(report)
        payload = json.loads(render_json(report))

        assert "AC counts" in text
        assert "Subsystem counts" in text
        assert "Malformed citations: 0" in text
        assert payload["citations_by_ac"] == {"US-023-AC1": 1}
        assert set(payload) == {
            "citations_by_ac",
            "citations_by_subsystem",
            "malformed_citations",
            "planned_ac_ids",
        }
        with redirect_stdout(io.StringIO()):
            assert (
                main(
                    [
                        "--root",
                        str(root),
                        "--format",
                        "json",
                        "--test-plan-dir",
                        str(TEST_PLAN_DIR),
                    ]
                )
                == 0
            )


def run_tests() -> None:
    test_valid_citations_and_subsystem_counts()
    test_malformed_covers_tokens()
    test_stp_parsing_ignores_non_ac_table_cells()
    test_renderers_and_cli_exit_codes()


if __name__ == "__main__":
    run_tests()
