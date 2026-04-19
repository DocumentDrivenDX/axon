from __future__ import annotations

import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
FEATURE_DIR = REPO_ROOT / "docs" / "helix" / "01-frame" / "features"
TRACEABILITY_DOC = (
    REPO_ROOT
    / "docs"
    / "helix"
    / "03-test"
    / "feature-story-e2e-traceability.md"
)


class FeatureStoryTraceabilityTests(unittest.TestCase):
    def test_every_feature_has_user_stories_and_traceability_row(self) -> None:
        traceability = TRACEABILITY_DOC.read_text(encoding="utf-8")

        for path in sorted(FEATURE_DIR.glob("FEAT-*.md")):
            text = path.read_text(encoding="utf-8")
            feature_id = path.name.split("-", 2)[0]

            self.assertIn(
                "## User Stories",
                text,
                f"{path.name} must keep user stories explicit",
            )
            self.assertRegex(
                traceability,
                rf"\|\s*{re.escape(feature_id)}\b",
                f"{feature_id} missing from feature traceability matrix",
            )

    def test_checked_acceptance_criteria_name_executable_coverage(self) -> None:
        for path in sorted(FEATURE_DIR.glob("FEAT-*.md")):
            lines = path.read_text(encoding="utf-8").splitlines()
            for idx, line in enumerate(lines):
                if not line.startswith("- [x]"):
                    continue

                block = [line]
                for following in lines[idx + 1 :]:
                    if following.startswith("- ["):
                        break
                    if following.startswith("### "):
                        break
                    if following.strip():
                        block.append(following)

                joined = " ".join(part.strip() for part in block)
                self.assertRegex(
                    joined,
                    r"\b(E2E|Test):",
                    f"{path.name}:{idx + 1} checked AC lacks E2E/Test reference",
                )


if __name__ == "__main__":
    unittest.main()
