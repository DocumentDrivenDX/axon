#!/usr/bin/env python3
"""Validate HELIX tracker measure timestamps against bead updated_at values."""

from __future__ import annotations

import argparse
from html.parser import HTMLParser
import json
import sys
from datetime import datetime
from pathlib import Path


def parse_timestamp(value: str) -> datetime:
    if value.endswith("Z"):
        value = f"{value[:-1]}+00:00"
    return datetime.fromisoformat(value)


class MeasureResultsTimestampParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._stack: list[str] = []
        self._top_level_measure_results_blocks: list[
            tuple[list[list[str]], str | None]
        ] = []
        self._open_measure_results_timestamps: list[list[str]] | None = None

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if not self._stack and tag == "measure-results":
            self._open_measure_results_timestamps = []
        elif (
            len(self._stack) == 1
            and self._stack[0] == "measure-results"
            and tag == "timestamp"
            and self._open_measure_results_timestamps is not None
        ):
            self._open_measure_results_timestamps.append([])
        self._stack.append(tag)

    def handle_startendtag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if not self._stack and tag == "measure-results":
            self._top_level_measure_results_blocks.append(
                ([], "self-closing <measure-results/>")
            )
        if (
            len(self._stack) == 1
            and self._stack[0] == "measure-results"
            and tag == "timestamp"
            and self._open_measure_results_timestamps is not None
        ):
            self._open_measure_results_timestamps.append([])

    def handle_endtag(self, tag: str) -> None:
        if (
            len(self._stack) == 1
            and self._stack[0] == "measure-results"
            and tag == "measure-results"
            and self._open_measure_results_timestamps is not None
        ):
            self._top_level_measure_results_blocks.append(
                (self._open_measure_results_timestamps, None)
            )
            self._open_measure_results_timestamps = None
        for index in range(len(self._stack) - 1, -1, -1):
            if self._stack[index] == tag:
                del self._stack[index:]
                break

    def handle_data(self, data: str) -> None:
        if (
            len(self._stack) == 2
            and self._stack[0] == "measure-results"
            and self._stack[1] == "timestamp"
            and self._open_measure_results_timestamps
        ):
            self._open_measure_results_timestamps[-1].append(data)

    def top_level_measure_results_timestamps(
        self,
    ) -> list[tuple[list[str], str | None]]:
        blocks = [
            (
                [
                    "".join(chunks).strip()
                    for chunks in block
                    if "".join(chunks).strip()
                ],
                malformed_reason,
            )
            for block, malformed_reason in self._top_level_measure_results_blocks
        ]
        if self._open_measure_results_timestamps is not None:
            blocks.append(([], "missing closing </measure-results>"))
        return blocks


def iter_measure_timestamps(notes: str) -> list[tuple[str | None, str | None]]:
    parser = MeasureResultsTimestampParser()
    parser.feed(notes)
    parser.close()

    timestamps: list[tuple[str | None, str | None]] = []
    for matches, malformed_reason in parser.top_level_measure_results_timestamps():
        if malformed_reason is not None:
            timestamps.append((None, malformed_reason))
            continue
        if len(matches) == 1:
            timestamps.append((matches[0], None))
        else:
            reason = "missing timestamp" if not matches else "multiple timestamps"
            timestamps.append((None, reason))
    return timestamps


def validate_tracker(path: Path, included_ids: set[str] | None = None) -> int:
    failures: list[str] = []
    matched_ids: set[str] = set()
    validated_counts: dict[str, int] = {}
    unvalidated_reasons: dict[str, str] = {}

    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            record = json.loads(line)
            notes = record.get("notes")
            updated_at = record.get("updated_at")
            bead_id = record.get("id", "<unknown>")

            if included_ids and bead_id not in included_ids:
                continue
            if included_ids:
                matched_ids.add(bead_id)
                validated_counts.setdefault(bead_id, 0)
            if not notes:
                if included_ids:
                    unvalidated_reasons[bead_id] = "missing notes"
                continue
            if not updated_at:
                if included_ids:
                    unvalidated_reasons[bead_id] = "missing updated_at"
                continue

            try:
                updated_at_dt = parse_timestamp(updated_at)
            except ValueError:
                failures.append(
                    f"{path}:{line_number}: bead {bead_id} has invalid updated_at {updated_at}"
                )
                if included_ids:
                    unvalidated_reasons[bead_id] = "invalid updated_at"
                continue

            measure_timestamps = iter_measure_timestamps(notes)
            for measure_timestamp, malformed_reason in measure_timestamps:
                if measure_timestamp is None:
                    failures.append(
                        (
                            f"{path}:{line_number}: bead {bead_id} has "
                            f"measure-results block with {malformed_reason}"
                        )
                    )
                    if included_ids:
                        unvalidated_reasons.setdefault(
                            bead_id, f"measure-results block with {malformed_reason}"
                        )
                    continue
                try:
                    measure_timestamp_dt = parse_timestamp(measure_timestamp)
                except ValueError:
                    failures.append(
                        (
                            f"{path}:{line_number}: bead {bead_id} has invalid "
                            f"measure timestamp {measure_timestamp}"
                        )
                    )
                    if included_ids:
                        unvalidated_reasons.setdefault(
                            bead_id, "invalid measure timestamp"
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
                    if included_ids:
                        unvalidated_reasons.setdefault(
                            bead_id, "measure timestamp later than updated_at"
                        )
                    continue
                if included_ids:
                    validated_counts[bead_id] += 1

            if included_ids and not measure_timestamps:
                unvalidated_reasons[bead_id] = (
                    "missing <measure-results> timestamp evidence"
                )

    if included_ids:
        missing_ids = sorted(included_ids - matched_ids)
        if missing_ids:
            failures.append(
                f"{path}: requested bead IDs not found: {', '.join(missing_ids)}"
            )
        zero_evidence_ids = sorted(
            bead_id for bead_id, count in validated_counts.items() if count == 0
        )
        if zero_evidence_ids:
            failures.append(
                (
                    f"{path}: requested bead IDs lacked validated measurement evidence: "
                    + ", ".join(
                        f"{bead_id} ({unvalidated_reasons.get(bead_id, 'no valid measure timestamps')})"
                        for bead_id in zero_evidence_ids
                    )
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
    included_ids = set(args.ids) or None
    return validate_tracker(Path(args.path), included_ids=included_ids)


if __name__ == "__main__":
    raise SystemExit(main())
