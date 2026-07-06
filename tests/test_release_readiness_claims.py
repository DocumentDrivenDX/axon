#!/usr/bin/env python3
"""Inventory release-readiness claims across the HELIX docs.

Makes release/readiness/version claims mechanically visible before a docs
sweep: release target claims, ready-to-use claims, PRD success criteria,
p99 latency claims, parking-lot deferred decisions, and DDx frontmatter
hash/staleness flags. This is an inventory, not a verdict — it does not
decide the release target or fail on inconsistent claims; it only makes
them discoverable in one place.

Usage:
    python3 tests/test_release_readiness_claims.py --format text
    python3 tests/test_release_readiness_claims.py --format json
    python3 -m unittest tests.test_release_readiness_claims
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import unittest
from pathlib import Path

THIS_FILE = Path(__file__).resolve()
REPO_ROOT = THIS_FILE.parents[1]
HELIX_DIR = REPO_ROOT / "docs" / "helix"
PRD_PATH = HELIX_DIR / "01-frame" / "prd.md"
PARKING_LOT_PATH = HELIX_DIR / "parking-lot.md"
CARGO_TOML_PATH = REPO_ROOT / "Cargo.toml"

VERSION_RE = re.compile(r"\bv?\d+\.\d+\.\d+\b")
RELEASE_SIGNAL_RE = re.compile(r"release|cargo\.toml|workspace version|\btags?\b", re.IGNORECASE)
READY_TO_USE_RE = re.compile(
    r"ready[- ]to[- ]use|production[- ]ready|ready for (?:production|use)"
    r"|pilot-ready|ga-ready",
    re.IGNORECASE,
)
P99_RE = re.compile(r"p99", re.IGNORECASE)
FRONTMATTER_RE = re.compile(r"\A---\n(.*?\n)---\n", re.DOTALL)
DDX_ID_RE = re.compile(r"^\s+id:\s*(\S+)\s*$", re.MULTILINE)
SELF_HASH_RE = re.compile(r"self_hash:\s*([0-9a-fA-F]{6,})")
REVIEWED_AT_RE = re.compile(r'reviewed_at:\s*"?([^"\n]+?)"?\s*$', re.MULTILINE)
STALE_REVIEW_RE = re.compile(r"TODO:\s*refresh review stamp", re.IGNORECASE)
SUCCESS_CRITERIA_ITEM_RE = re.compile(r"^-\s\[([ xX])\]\s+(.+)$")


def iter_markdown_files() -> list[Path]:
    return sorted(HELIX_DIR.rglob("*.md"))


def relpath(path: Path) -> str:
    return str(path.relative_to(REPO_ROOT))


def cargo_toml_version() -> str | None:
    text = CARGO_TOML_PATH.read_text(encoding="utf-8")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    return match.group(1) if match else None


def _scan_lines(pattern: re.Pattern[str]) -> list[dict]:
    matches = []
    for path in iter_markdown_files():
        for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if pattern.search(line):
                matches.append({"file": relpath(path), "line": line_no, "text": line.strip()})
    return matches


def collect_release_target_claims() -> dict:
    claims = []
    for path in iter_markdown_files():
        for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if VERSION_RE.search(line) and RELEASE_SIGNAL_RE.search(line):
                claims.append({"file": relpath(path), "line": line_no, "text": line.strip()})
    return {"cargo_toml_version": cargo_toml_version(), "claims": claims}


def collect_ready_to_use_claims() -> list[dict]:
    return _scan_lines(READY_TO_USE_RE)


def collect_p99_claims() -> list[dict]:
    return _scan_lines(P99_RE)


def collect_prd_success_criteria() -> list[dict]:
    lines = PRD_PATH.read_text(encoding="utf-8").splitlines()

    start = None
    for idx, line in enumerate(lines):
        if line.strip() == "## Success Criteria":
            start = idx + 1
            break
    if start is None:
        return []

    items = []
    idx = start
    while idx < len(lines):
        line = lines[idx]
        if line.startswith("## "):
            break
        match = SUCCESS_CRITERIA_ITEM_RE.match(line)
        if match is None:
            idx += 1
            continue

        checked = match.group(1).lower() == "x"
        text_parts = [match.group(2).strip()]
        follow = idx + 1
        while (
            follow < len(lines)
            and lines[follow].strip()
            and not SUCCESS_CRITERIA_ITEM_RE.match(lines[follow])
            and not lines[follow].startswith("#")
        ):
            text_parts.append(lines[follow].strip())
            follow += 1

        items.append({"line": idx + 1, "checked": checked, "text": " ".join(text_parts)})
        idx = follow

    return items


def collect_parking_lot_deferred_decisions() -> list[dict]:
    lines = PARKING_LOT_PATH.read_text(encoding="utf-8").splitlines()

    items: list[dict] = []
    current: dict | None = None

    def flush() -> None:
        if current is not None:
            items.append(
                {
                    "title": current["title"],
                    "line": current["line"],
                    "type": current["fields"].get("Type"),
                    "revisit_trigger": current["fields"].get("Revisit Trigger"),
                }
            )

    for idx, line in enumerate(lines):
        heading_match = re.match(r"^###\s+(.+)$", line)
        if heading_match:
            flush()
            current = {"title": heading_match.group(1).strip(), "line": idx + 1, "fields": {}}
            continue
        if line.startswith("## "):
            flush()
            current = None
            continue
        if current is not None:
            field_match = re.match(r"^-\s+\*\*(.+?)\*\*:\s*(.+)$", line)
            if field_match:
                current["fields"][field_match.group(1).strip()] = field_match.group(2).strip()

    flush()
    return items


def collect_ddx_frontmatter() -> list[dict]:
    entries = []
    for path in iter_markdown_files():
        text = path.read_text(encoding="utf-8")
        match = FRONTMATTER_RE.match(text)
        if match is None or "ddx:" not in match.group(1):
            continue

        block = match.group(1)
        id_match = DDX_ID_RE.search(block)
        hash_match = SELF_HASH_RE.search(block)
        reviewed_match = REVIEWED_AT_RE.search(block)

        entries.append(
            {
                "file": relpath(path),
                "id": id_match.group(1) if id_match else None,
                "self_hash": hash_match.group(1) if hash_match else None,
                "reviewed_at": reviewed_match.group(1) if reviewed_match else None,
                "stale_review_stamp": bool(STALE_REVIEW_RE.search(text)),
            }
        )
    return entries


def build_inventory() -> dict:
    return {
        "release_target_claims": collect_release_target_claims(),
        "ready_to_use_claims": collect_ready_to_use_claims(),
        "prd_success_criteria": collect_prd_success_criteria(),
        "p99_claims": collect_p99_claims(),
        "deferred_decisions": collect_parking_lot_deferred_decisions(),
        "ddx_frontmatter_hashes": collect_ddx_frontmatter(),
    }


def render_text_report(report: dict) -> str:
    lines: list[str] = []

    release = report["release_target_claims"]
    lines.append(
        f"Release target claims: {len(release['claims'])} "
        f"(Cargo.toml version = {release['cargo_toml_version']})"
    )
    for claim in release["claims"]:
        lines.append(f"  {claim['file']}:{claim['line']}: {claim['text']}")

    ready = report["ready_to_use_claims"]
    lines.append(f"\nReady-to-use claims: {len(ready)}")
    for claim in ready:
        lines.append(f"  {claim['file']}:{claim['line']}: {claim['text']}")

    criteria = report["prd_success_criteria"]
    checked = sum(1 for item in criteria if item["checked"])
    lines.append(f"\nPRD success criteria: {checked}/{len(criteria)} checked")
    for item in criteria:
        box = "x" if item["checked"] else " "
        lines.append(f"  [{box}] prd.md:{item['line']}: {item['text']}")

    p99 = report["p99_claims"]
    lines.append(f"\np99 claims: {len(p99)}")
    for claim in p99:
        lines.append(f"  {claim['file']}:{claim['line']}: {claim['text']}")

    deferred = report["deferred_decisions"]
    lines.append(f"\nParking-lot deferred/future decisions: {len(deferred)}")
    for item in deferred:
        lines.append(
            f"  parking-lot.md:{item['line']}: {item['title']} ({item['type']})"
        )

    ddx = report["ddx_frontmatter_hashes"]
    stale = [entry for entry in ddx if entry["stale_review_stamp"]]
    lines.append(
        f"\nDDx frontmatter/hash entries: {len(ddx)} ({len(stale)} flagged stale)"
    )
    for entry in stale:
        lines.append(
            f"  {entry['file']}: id={entry['id']} self_hash={entry['self_hash']} stale=True"
        )

    return "\n".join(lines)


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Inventory release-readiness claims across HELIX docs."
    )
    parser.add_argument(
        "--format",
        choices=("text", "json"),
        default="text",
        help="Output format for the claim inventory",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_arg_parser().parse_args(argv)
    report = build_inventory()
    if args.format == "json":
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(render_text_report(report))
    return 0


class ReleaseReadinessClaimInventoryTests(unittest.TestCase):
    def test_inventory_collects_every_category(self) -> None:
        report = build_inventory()
        self.assertTrue(report["release_target_claims"]["claims"])
        self.assertTrue(report["ready_to_use_claims"])
        self.assertTrue(report["prd_success_criteria"])
        self.assertTrue(report["p99_claims"])
        self.assertTrue(report["deferred_decisions"])
        self.assertTrue(report["ddx_frontmatter_hashes"])

    def test_prd_success_criteria_have_text_and_checked_flag(self) -> None:
        for item in build_inventory()["prd_success_criteria"]:
            self.assertIn("checked", item)
            self.assertTrue(item["text"])

    def test_ddx_frontmatter_entries_have_ids(self) -> None:
        for entry in build_inventory()["ddx_frontmatter_hashes"]:
            self.assertIsNotNone(entry["id"], entry["file"])

    def test_report_is_json_serializable(self) -> None:
        json.dumps(build_inventory())

    def test_cli_text_and_json_formats_exit_zero(self) -> None:
        for fmt in ("text", "json"):
            result = subprocess.run(
                [sys.executable, str(THIS_FILE), "--format", fmt],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(result.stdout.strip())
            if fmt == "json":
                json.loads(result.stdout)


if __name__ == "__main__":
    if any(arg == "--format" or arg.startswith("--format=") for arg in sys.argv[1:]):
        raise SystemExit(main(sys.argv[1:]))
    unittest.main()
