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
    def __init__(self, block_tag: str) -> None:
        super().__init__(convert_charrefs=True)
        self._block_tag = block_tag
        self._stack: list[str] = []
        self._top_level_blocks: list[tuple[list[list[str]], str | None]] = []
        self._open_block_timestamps: list[list[str]] | None = None

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if not self._stack and tag == self._block_tag:
            self._open_block_timestamps = []
        elif (
            len(self._stack) == 1
            and self._stack[0] == self._block_tag
            and tag == "timestamp"
            and self._open_block_timestamps is not None
        ):
            self._open_block_timestamps.append([])
        self._stack.append(tag)

    def handle_startendtag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if not self._stack and tag == self._block_tag:
            self._top_level_blocks.append(
                ([], f"self-closing <{self._block_tag}/>")
            )
        if (
            len(self._stack) == 1
            and self._stack[0] == self._block_tag
            and tag == "timestamp"
            and self._open_block_timestamps is not None
        ):
            self._open_block_timestamps.append([])

    def handle_endtag(self, tag: str) -> None:
        if (
            len(self._stack) == 1
            and self._stack[0] == self._block_tag
            and tag == self._block_tag
            and self._open_block_timestamps is not None
        ):
            self._top_level_blocks.append((self._open_block_timestamps, None))
            self._open_block_timestamps = None
        for index in range(len(self._stack) - 1, -1, -1):
            if self._stack[index] == tag:
                del self._stack[index:]
                break

    def handle_data(self, data: str) -> None:
        if (
            len(self._stack) == 2
            and self._stack[0] == self._block_tag
            and self._stack[1] == "timestamp"
            and self._open_block_timestamps
        ):
            self._open_block_timestamps[-1].append(data)

    def top_level_timestamps(self) -> list[tuple[list[str], str | None]]:
        blocks = [
            (
                [
                    "".join(chunks).strip()
                    for chunks in block
                    if "".join(chunks).strip()
                ],
                malformed_reason,
            )
            for block, malformed_reason in self._top_level_blocks
        ]
        if self._open_block_timestamps is not None:
            blocks.append(([], f"missing closing </{self._block_tag}>"))
        return blocks


def iter_top_level_timestamps(
    notes: str, block_tag: str
) -> list[tuple[str | None, str | None]]:
    parser = MeasureResultsTimestampParser(block_tag)
    parser.feed(notes)
    parser.close()

    timestamps: list[tuple[str | None, str | None]] = []
    for matches, malformed_reason in parser.top_level_timestamps():
        if malformed_reason is not None:
            timestamps.append((None, malformed_reason))
            continue
        if len(matches) == 1:
            timestamps.append((matches[0], None))
        else:
            reason = "missing timestamp" if not matches else "multiple timestamps"
            timestamps.append((None, reason))
    return timestamps


def iter_measure_timestamps(notes: str) -> list[tuple[str | None, str | None]]:
    return iter_top_level_timestamps(notes, "measure-results")


def iter_report_summary_timestamps(
    notes: str,
) -> list[tuple[str | None, str | None]]:
    return iter_top_level_timestamps(notes, "report-summary")


def validate_timestamps(
    *,
    path: Path,
    line_number: int,
    bead_id: str,
    updated_at: str,
    updated_at_dt: datetime,
    block_label: str,
    timestamps: list[tuple[str | None, str | None]],
    failures: list[str],
    validated_counts: dict[str, int] | None = None,
    unvalidated_reasons: dict[str, str] | None = None,
) -> None:
    for block_timestamp, malformed_reason in timestamps:
        if block_timestamp is None:
            failures.append(
                (
                    f"{path}:{line_number}: bead {bead_id} has "
                    f"{block_label} block with {malformed_reason}"
                )
            )
            if unvalidated_reasons is not None:
                unvalidated_reasons.setdefault(
                    bead_id, f"{block_label} block with {malformed_reason}"
                )
            continue
        try:
            block_timestamp_dt = parse_timestamp(block_timestamp)
        except ValueError:
            failures.append(
                (
                    f"{path}:{line_number}: bead {bead_id} has invalid "
                    f"{block_label} timestamp {block_timestamp}"
                )
            )
            if unvalidated_reasons is not None:
                unvalidated_reasons.setdefault(
                    bead_id, f"invalid {block_label} timestamp"
                )
            continue
        if block_timestamp_dt > updated_at_dt:
            failures.append(
                (
                    f"{path}:{line_number}: bead {bead_id} has "
                    f"{block_label} timestamp {block_timestamp} later than "
                    f"updated_at {updated_at}"
                )
            )
            if unvalidated_reasons is not None:
                unvalidated_reasons.setdefault(
                    bead_id, f"{block_label} timestamp later than updated_at"
                )
            continue
        if validated_counts is not None:
            validated_counts[bead_id] += 1


def validate_tracker(
    path: Path,
    included_ids: set[str] | None = None,
    require_report_summary: bool = False,
) -> int:
    failures: list[str] = []
    matched_ids: set[str] = set()
    validated_counts: dict[str, int] = {}
    validated_report_counts: dict[str, int] = {}
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
                validated_report_counts.setdefault(bead_id, 0)
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
            validate_timestamps(
                path=path,
                line_number=line_number,
                bead_id=bead_id,
                updated_at=updated_at,
                updated_at_dt=updated_at_dt,
                block_label="measure-results",
                timestamps=measure_timestamps,
                failures=failures,
                validated_counts=validated_counts if included_ids else None,
                unvalidated_reasons=unvalidated_reasons if included_ids else None,
            )
            report_summary_timestamps = iter_report_summary_timestamps(notes)

            if included_ids and not measure_timestamps:
                unvalidated_reasons[bead_id] = (
                    "missing <measure-results> timestamp evidence"
                )
            if included_ids and require_report_summary and not report_summary_timestamps:
                unvalidated_reasons[bead_id] = (
                    "missing <report-summary> timestamp evidence"
                )
            if included_ids and require_report_summary and report_summary_timestamps:
                validate_timestamps(
                    path=path,
                    line_number=line_number,
                    bead_id=bead_id,
                    updated_at=updated_at,
                    updated_at_dt=updated_at_dt,
                    block_label="report-summary",
                    timestamps=report_summary_timestamps,
                    failures=failures,
                    validated_counts=validated_report_counts,
                    unvalidated_reasons=unvalidated_reasons,
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
        if require_report_summary:
            zero_report_ids = sorted(
                bead_id
                for bead_id, count in validated_report_counts.items()
                if count == 0
            )
            if zero_report_ids:
                failures.append(
                    (
                        f"{path}: requested bead IDs lacked validated report-summary evidence: "
                        + ", ".join(
                            f"{bead_id} ({unvalidated_reasons.get(bead_id, 'no valid report-summary timestamps')})"
                            for bead_id in zero_report_ids
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
    parser.add_argument(
        "--require-report-summary",
        action="store_true",
        help="Require requested bead IDs to have a valid top-level report-summary timestamp",
    )
    args = parser.parse_args()
    included_ids = set(args.ids) or None
    return validate_tracker(
        Path(args.path),
        included_ids=included_ids,
        require_report_summary=args.require_report_summary,
    )


if __name__ == "__main__":
    raise SystemExit(main())
