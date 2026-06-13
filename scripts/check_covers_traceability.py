#!/usr/bin/env python3
"""Report executable @covers traceability against HELIX story test plans."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import re
import sys
from pathlib import Path
from typing import Iterable


CANONICAL_COVERS_RE = re.compile(r"\s+(US-\d+-AC\d+)\b")
PLANNED_AC_RE = re.compile(r"\|\s*(US-\d+-AC\d+)\s*\|")
SUPPORTED_SUFFIXES = {
    ".cjs",
    ".js",
    ".jsx",
    ".mjs",
    ".rs",
    ".svelte",
    ".ts",
    ".tsx",
}
SKIPPED_PARTS = {
    ".git",
    ".svelte-kit",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "playwright-report",
    "target",
    "test-results",
}


@dataclass(frozen=True)
class Citation:
    ac_id: str
    path: str
    line: int
    subsystem: str


@dataclass(frozen=True)
class MalformedCitation:
    path: str
    line: int
    token: str


@dataclass(frozen=True)
class TraceabilityReport:
    citations: list[Citation]
    malformed_citations: list[MalformedCitation]
    planned_ac_ids: list[str]

    @property
    def citations_by_ac(self) -> dict[str, int]:
        counts: dict[str, int] = {}
        for citation in self.citations:
            counts[citation.ac_id] = counts.get(citation.ac_id, 0) + 1
        return dict(sorted(counts.items(), key=lambda item: ac_sort_key(item[0])))

    @property
    def citations_by_subsystem(self) -> dict[str, int]:
        counts: dict[str, int] = {}
        for citation in self.citations:
            counts[citation.subsystem] = counts.get(citation.subsystem, 0) + 1
        return dict(sorted(counts.items()))


def ac_sort_key(ac_id: str) -> tuple[int, int, str]:
    match = re.fullmatch(r"US-(\d+)-AC(\d+)", ac_id)
    if not match:
        return (sys.maxsize, sys.maxsize, ac_id)
    return (int(match.group(1)), int(match.group(2)), ac_id)


def should_scan_file(path: Path) -> bool:
    return path.suffix in SUPPORTED_SUFFIXES and not (
        set(path.parts) & SKIPPED_PARTS
    )


def iter_source_files(root: Path, scan_roots: Iterable[Path]) -> Iterable[Path]:
    for scan_root in scan_roots:
        absolute_root = root / scan_root
        if not absolute_root.exists():
            continue
        for path in absolute_root.rglob("*"):
            if path.is_file() and should_scan_file(path.relative_to(root)):
                yield path


def subsystem_for_path(relative_path: Path) -> str:
    parts = relative_path.parts
    if len(parts) >= 2 and parts[0] == "crates":
        return f"crates/{parts[1]}"
    if parts[:2] == ("sdk", "typescript"):
        return "sdk/typescript"
    if parts and parts[0] == "ui":
        return "ui"
    return parts[0] if parts else "."


def token_excerpt(line: str, start: int) -> str:
    remainder = line[start:].strip()
    if not remainder:
        return "@covers"
    return remainder.split(maxsplit=1)[0]


def parse_citations(root: Path, scan_roots: Iterable[Path]) -> tuple[
    list[Citation], list[MalformedCitation]
]:
    citations: list[Citation] = []
    malformed: list[MalformedCitation] = []

    for path in iter_source_files(root, scan_roots):
        relative_path = path.relative_to(root)
        subsystem = subsystem_for_path(relative_path)
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for line_number, line in enumerate(lines, start=1):
            for marker in re.finditer(r"@covers", line):
                match = CANONICAL_COVERS_RE.match(line[marker.end() :])
                if match:
                    citations.append(
                        Citation(
                            ac_id=match.group(1),
                            path=relative_path.as_posix(),
                            line=line_number,
                            subsystem=subsystem,
                        )
                    )
                else:
                    malformed.append(
                        MalformedCitation(
                            path=relative_path.as_posix(),
                            line=line_number,
                            token=token_excerpt(line, marker.start()),
                        )
                    )
    return citations, malformed


def parse_planned_ac_ids(root: Path, test_plan_dir: Path) -> list[str]:
    plan_root = root / test_plan_dir
    ac_ids: set[str] = set()
    if not plan_root.exists():
        return []

    for path in sorted(plan_root.glob("STP-*.md")):
        text = path.read_text(encoding="utf-8")
        ac_ids.update(PLANNED_AC_RE.findall(text))
    return sorted(ac_ids, key=ac_sort_key)


def collect_traceability(
    *,
    root: Path,
    scan_roots: Iterable[Path],
    test_plan_dir: Path,
) -> TraceabilityReport:
    citations, malformed = parse_citations(root, scan_roots)
    planned_ac_ids = parse_planned_ac_ids(root, test_plan_dir)
    return TraceabilityReport(
        citations=citations,
        malformed_citations=malformed,
        planned_ac_ids=planned_ac_ids,
    )


def render_text(report: TraceabilityReport) -> str:
    lines = ["AC counts"]
    if report.citations_by_ac:
        lines.extend(
            f"  {ac_id}: {count}"
            for ac_id, count in report.citations_by_ac.items()
        )
    else:
        lines.append("  (none)")

    lines.append("Subsystem counts")
    if report.citations_by_subsystem:
        lines.extend(
            f"  {subsystem}: {count}"
            for subsystem, count in report.citations_by_subsystem.items()
        )
    else:
        lines.append("  (none)")

    lines.append(f"Malformed citations: {len(report.malformed_citations)}")
    for malformed in report.malformed_citations:
        lines.append(
            f"  {malformed.path}:{malformed.line}: malformed {malformed.token}"
        )
    lines.append(f"Planned AC IDs: {len(report.planned_ac_ids)}")
    return "\n".join(lines)


def render_json(report: TraceabilityReport) -> str:
    payload = {
        "citations_by_ac": report.citations_by_ac,
        "citations_by_subsystem": report.citations_by_subsystem,
        "malformed_citations": [
            {
                "path": malformed.path,
                "line": malformed.line,
                "token": malformed.token,
            }
            for malformed in report.malformed_citations
        ],
        "planned_ac_ids": report.planned_ac_ids,
    }
    return json.dumps(payload, indent=2, sort_keys=True)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Scan executable sources for canonical @covers AC citations."
    )
    parser.add_argument(
        "--format",
        choices=("json", "text"),
        default="text",
        help="output format",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="repository root",
    )
    parser.add_argument(
        "--scan-root",
        action="append",
        type=Path,
        dest="scan_roots",
        help="source root to scan, relative to --root",
    )
    parser.add_argument(
        "--test-plan-dir",
        type=Path,
        default=Path("docs/helix/03-test/test-plans"),
        help="test-plan directory, relative to --root",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    scan_roots = args.scan_roots or [
        Path("crates"),
        Path("ui"),
        Path("sdk/typescript"),
    ]
    root = args.root.resolve()

    report = collect_traceability(
        root=root,
        scan_roots=scan_roots,
        test_plan_dir=args.test_plan_dir,
    )
    if args.format == "json":
        print(render_json(report))
    else:
        print(render_text(report))
    return 1 if report.malformed_citations else 0


if __name__ == "__main__":
    raise SystemExit(main())
