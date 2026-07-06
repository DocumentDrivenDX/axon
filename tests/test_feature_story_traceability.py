from __future__ import annotations

import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
FEATURE_DIR = REPO_ROOT / "docs" / "helix" / "01-frame" / "features"
USER_STORY_DIR = REPO_ROOT / "docs" / "helix" / "01-frame" / "user-stories"

USER_STORY_LINK_RE = re.compile(r"\((\.\./user-stories/US-\d+-[a-z0-9-]+\.md)\)")


def _user_stories_section(text: str) -> str:
    lines = text.splitlines()
    for idx, line in enumerate(lines):
        if line.strip() != "## User Stories":
            continue
        block: list[str] = []
        for following in lines[idx + 1 :]:
            if following.startswith("## "):
                break
            block.append(following)
        return "\n".join(block)
    return ""


class FeatureStoryTraceabilityTests(unittest.TestCase):
    """Traceability authority is the STP + ``@covers`` model (see
    docs/helix/03-test/test-plan.md §3 and
    scripts/check_covers_traceability.py); these tests only guard the
    upstream link integrity between features and the stories that carry
    their acceptance criteria (ADR-009)."""

    def test_every_feature_declares_user_stories_section(self) -> None:
        for path in sorted(FEATURE_DIR.glob("FEAT-*.md")):
            text = path.read_text(encoding="utf-8")
            if "**Status**: superseded" in text:
                continue
            self.assertIn(
                "## User Stories",
                text,
                f"{path.name} must keep a '## User Stories' section "
                "(acceptance criteria live in stories, not features, per ADR-009)",
            )

    def test_linked_user_stories_resolve_to_real_story_files(self) -> None:
        for path in sorted(FEATURE_DIR.glob("FEAT-*.md")):
            text = path.read_text(encoding="utf-8")
            section = _user_stories_section(text)
            for relative_link in USER_STORY_LINK_RE.findall(section):
                story_path = (FEATURE_DIR / relative_link).resolve()
                self.assertTrue(
                    story_path.is_file(),
                    f"{path.name} links to {relative_link}, which does not "
                    f"exist under {USER_STORY_DIR}",
                )


if __name__ == "__main__":
    unittest.main()
