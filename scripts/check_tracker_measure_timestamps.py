#!/usr/bin/env python3
"""Validate HELIX tracker measure timestamps against bead updated_at values."""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime
from pathlib import Path


MEASURE_TIMESTAMP_RE = re.compile(
    r"<measure-results>.*?<timestamp>([^<]+)</timestamp>", re.DOTALL
)


def parse_timestamp(value: str) -> datetime:
    if value.endswith("Z"):
        value = f"{value[:-1]}+00:00"
    return datetime.fromisoformat(value)


def validate_tracker(path: Path) -> int:
    failures: list[str] = []

    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            record = json.loads(line)
            notes = record.get("notes")
            updated_at = record.get("updated_at")
            bead_id = record.get("id", "<unknown>")
            included_ids = getattr(validate_tracker, "included_ids", None)

            if not notes or not updated_at:
                continue
            if included_ids and bead_id not in included_ids:
                continue

            try:
                updated_at_dt = parse_timestamp(updated_at)
            except ValueError:
                failures.append(
                    f"{path}:{line_number}: bead {bead_id} has invalid updated_at {updated_at}"
                )
                continue

            for match in MEASURE_TIMESTAMP_RE.finditer(notes):
                measure_timestamp = match.group(1).strip()
                try:
                    measure_timestamp_dt = parse_timestamp(measure_timestamp)
                except ValueError:
                    failures.append(
                        (
                            f"{path}:{line_number}: bead {bead_id} has invalid "
                            f"measure timestamp {measure_timestamp}"
                        )
                    )
                    continue
                if measure_timestamp_dt > updated_at_dt:
                    failures.append(
                        (
                            f"{path}:{line_number}: bead {bead_id} has "
                            f"measure timestamp {measure_timestamp} later than "
                            f"updated_at {updated_at}"
                        )
                    )

    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1

    print(f"{path}: all measure timestamps are <= updated_at")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate HELIX tracker measure timestamps."
    )
    parser.add_argument(
        "path",
        nargs="?",
        default=".ddx/beads.jsonl",
        help="Path to the tracker JSONL file",
    )
    parser.add_argument(
        "--id",
        action="append",
        dest="ids",
        default=[],
        help="Restrict validation to specific bead IDs (repeatable)",
    )
    args = parser.parse_args()
    validate_tracker.included_ids = set(args.ids)
    return validate_tracker(Path(args.path))


if __name__ == "__main__":
    raise SystemExit(main())
