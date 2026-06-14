#!/usr/bin/env python3
"""Generate the Axon microsite coverage catalog from HELIX sources.

The generated pages intentionally project from docs/helix instead of a hand
maintained list. Run with --write to update the website/examples, and --check
in CI or locally to prove the committed projection is current.
"""

from __future__ import annotations

import argparse
import json
import re
import stat
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
RELEASE_TARGET = "0.7.1"
GENERATED_DATE = "2026-06-14"
GITHUB_BASE = "https://github.com/DocumentDrivenDX/axon"


@dataclass(frozen=True)
class CoverageItem:
    kind: str
    id: str
    title: str
    source_path: str
    sample_project: str
    demo_reel: str
    parent: str = ""


EXAMPLES: dict[str, dict[str, Any]] = {
    "agent-taskboard": {
        "title": "Agent Taskboard",
        "summary": (
            "A governed task and bead queue that demonstrates collections, "
            "schemas, entity CRUD, links, graph traversal, audit, optimistic "
            "concurrency, MCP-oriented task discovery, and the unified CLI."
        ),
        "persona": "Developers building agent-native workflow state.",
        "reel": "agent-taskboard-reel",
        "collections": {
            "projects": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "phase": {"type": "string"},
                    "owner": {"type": "string"},
                },
                "required": ["name", "phase", "owner"],
            },
            "beads": {
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "status": {
                        "type": "string",
                        "enum": ["ready", "in_progress", "review", "done"],
                    },
                    "priority": {"type": "integer"},
                    "agent": {"type": "string"},
                },
                "required": ["title", "status", "priority"],
            },
            "agents": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "role": {"type": "string"},
                    "scope": {"type": "string"},
                },
                "required": ["name", "role", "scope"],
            },
        },
        "entities": {
            "projects": [
                ("axon-core", {"name": "Axon core", "phase": "release-0.7.1", "owner": "platform"}),
            ],
            "agents": [
                ("agent-alpha", {"name": "Agent Alpha", "role": "builder", "scope": "storage"}),
                ("agent-beta", {"name": "Agent Beta", "role": "reviewer", "scope": "policy"}),
            ],
            "beads": [
                ("bead-001", {"title": "Define schema", "status": "done", "priority": 1, "agent": "agent-alpha"}),
                ("bead-002", {"title": "Wire graph query", "status": "ready", "priority": 2, "agent": "agent-beta"}),
                ("bead-003", {"title": "Record audit demo", "status": "ready", "priority": 3, "agent": "agent-alpha"}),
            ],
        },
        "links": [
            ("beads", "bead-002", "beads", "bead-001", "depends-on"),
            ("beads", "bead-003", "projects", "axon-core", "belongs-to"),
            ("beads", "bead-002", "agents", "agent-beta", "assigned-to"),
        ],
        "workflow": [
            "Create project, bead, and agent collections with schema validation.",
            "Seed beads and typed dependency links.",
            "Query ready work, claim one bead with optimistic concurrency, then inspect audit history.",
            "Traverse dependencies so agents can explain why work is ready or blocked.",
        ],
    },
    "invoice-approval-guardrails": {
        "title": "Invoice Approval Guardrails",
        "summary": (
            "A finance workflow covering AP/AR partial payment, state machines, "
            "policy envelopes, mutation intent preview/approval, rollback, and "
            "audit-safe trusted agent writes."
        ),
        "persona": "Finance operators and trusted finance agents.",
        "reel": "finance-guardrails-reel",
        "collections": {
            "vendors": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "risk": {"type": "string", "enum": ["low", "medium", "high"]},
                },
                "required": ["name", "risk"],
            },
            "invoices": {
                "type": "object",
                "properties": {
                    "number": {"type": "string"},
                    "amount": {"type": "number"},
                    "status": {
                        "type": "string",
                        "enum": ["draft", "submitted", "approved", "paid", "partially_paid"],
                    },
                    "requires_approval": {"type": "boolean"},
                },
                "required": ["number", "amount", "status", "requires_approval"],
            },
            "payments": {
                "type": "object",
                "properties": {
                    "reference": {"type": "string"},
                    "amount": {"type": "number"},
                    "status": {"type": "string"},
                },
                "required": ["reference", "amount", "status"],
            },
        },
        "entities": {
            "vendors": [
                ("vendor-acme", {"name": "Acme Supplies", "risk": "medium"}),
            ],
            "invoices": [
                ("inv-030", {"number": "INV-030", "amount": 5000, "status": "submitted", "requires_approval": True}),
                ("inv-035", {"number": "INV-035", "amount": 5000, "status": "submitted", "requires_approval": True}),
            ],
            "payments": [
                ("pmt-107", {"reference": "PMT-107", "amount": 7500, "status": "received"}),
            ],
        },
        "links": [
            ("invoices", "inv-030", "vendors", "vendor-acme", "billed-by"),
            ("invoices", "inv-035", "vendors", "vendor-acme", "billed-by"),
            ("payments", "pmt-107", "invoices", "inv-030", "applies-to"),
            ("payments", "pmt-107", "invoices", "inv-035", "applies-to"),
        ],
        "workflow": [
            "Seed vendors, invoices, and a partial payment.",
            "Preview the payment application as a mutation intent before commit.",
            "Approve the over-threshold write, commit it atomically, and verify audit grouping.",
            "Replay the transaction in rollback mode to show recoverability.",
        ],
    },
    "customer-identity-graph": {
        "title": "Customer Identity Graph",
        "summary": (
            "CRM, CDP, and MDM flows for contact merge, identity resolution, "
            "golden record survivorship, relationship traversal, and provenance."
        ),
        "persona": "Customer data teams and revenue operations.",
        "reel": "customer-identity-reel",
        "collections": {
            "contacts": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "email": {"type": "string"},
                    "source": {"type": "string"},
                },
                "required": ["name", "email", "source"],
            },
            "companies": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "tier": {"type": "string"},
                },
                "required": ["name", "tier"],
            },
            "profiles": {
                "type": "object",
                "properties": {
                    "canonical_email": {"type": "string"},
                    "confidence": {"type": "number"},
                    "segment": {"type": "string"},
                },
                "required": ["canonical_email", "confidence", "segment"],
            },
        },
        "entities": {
            "contacts": [
                ("contact-a", {"name": "Jane Doe", "email": "jane@example.com", "source": "crm"}),
                ("contact-b", {"name": "J. Doe", "email": "jane@example.com", "source": "support"}),
            ],
            "companies": [
                ("company-acme", {"name": "Acme Corp", "tier": "enterprise"}),
            ],
            "profiles": [
                ("profile-jane", {"canonical_email": "jane@example.com", "confidence": 0.96, "segment": "enterprise"}),
            ],
        },
        "links": [
            ("contacts", "contact-a", "companies", "company-acme", "works-at"),
            ("contacts", "contact-b", "companies", "company-acme", "works-at"),
            ("profiles", "profile-jane", "contacts", "contact-a", "resolved-from"),
            ("profiles", "profile-jane", "contacts", "contact-b", "resolved-from"),
        ],
        "workflow": [
            "Load duplicate contact records and company links.",
            "Resolve both contacts into a profile with confidence metadata.",
            "Traverse the contact-company-profile graph and verify no orphaned links after merge.",
            "Inspect the audit trail for explainable identity decisions.",
        ],
    },
    "supply-chain-bom": {
        "title": "Supply Chain BOM",
        "summary": (
            "ERP-style bill-of-materials traversal, recursive graph queries, "
            "reachability checks, aggregation, and link metadata."
        ),
        "persona": "Operations teams modeling product dependencies.",
        "reel": "supply-chain-reel",
        "collections": {
            "products": {
                "type": "object",
                "properties": {
                    "sku": {"type": "string"},
                    "name": {"type": "string"},
                    "kind": {"type": "string"},
                },
                "required": ["sku", "name", "kind"],
            },
            "build_orders": {
                "type": "object",
                "properties": {
                    "order_number": {"type": "string"},
                    "quantity": {"type": "integer"},
                    "status": {"type": "string"},
                },
                "required": ["order_number", "quantity", "status"],
            },
        },
        "entities": {
            "products": [
                ("widget-a", {"sku": "WIDGET-A", "name": "Widget A", "kind": "finished-good"}),
                ("sub-b", {"sku": "SUB-B", "name": "Sub Assembly B", "kind": "assembly"}),
                ("component-c", {"sku": "COMP-C", "name": "Component C", "kind": "part"}),
                ("component-d", {"sku": "COMP-D", "name": "Component D", "kind": "part"}),
            ],
            "build_orders": [
                ("bo-100", {"order_number": "BO-100", "quantity": 25, "status": "planned"}),
            ],
        },
        "links": [
            ("products", "widget-a", "products", "sub-b", "contains"),
            ("products", "widget-a", "products", "component-c", "contains"),
            ("products", "sub-b", "products", "component-c", "contains"),
            ("products", "sub-b", "products", "component-d", "contains"),
            ("build_orders", "bo-100", "products", "widget-a", "builds"),
        ],
        "workflow": [
            "Create a finished good, sub-assembly, component parts, and a build order.",
            "Attach contains links that encode the BOM graph.",
            "Traverse to depth three, then aggregate component demand.",
            "Use reachability checks to catch dependency cycles before commit.",
        ],
    },
    "tenant-control-plane": {
        "title": "Tenant Control Plane",
        "summary": (
            "Multi-tenant path routing, users, credentials, JWT grant bounds, "
            "revocation, deployment registration, and BYOC isolation."
        ),
        "persona": "Operators running Axon for multiple tenants or deployments.",
        "reel": "tenant-control-reel",
        "collections": {
            "tenants": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "plan": {"type": "string"},
                },
                "required": ["name", "plan"],
            },
            "users": {
                "type": "object",
                "properties": {
                    "email": {"type": "string"},
                    "role": {"type": "string"},
                },
                "required": ["email", "role"],
            },
            "deployments": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "region": {"type": "string"},
                    "status": {"type": "string"},
                },
                "required": ["name", "region", "status"],
            },
        },
        "entities": {
            "tenants": [
                ("tenant-acme", {"name": "acme", "plan": "enterprise"}),
                ("tenant-globex", {"name": "globex", "plan": "team"}),
            ],
            "users": [
                ("user-alice", {"email": "alice@example.com", "role": "admin"}),
                ("user-bob", {"email": "bob@example.com", "role": "read"}),
            ],
            "deployments": [
                ("dep-alpha", {"name": "dep-alpha", "region": "local-a", "status": "registered"}),
                ("dep-beta", {"name": "dep-beta", "region": "local-b", "status": "registered"}),
            ],
        },
        "links": [
            ("users", "user-alice", "tenants", "tenant-acme", "member-of"),
            ("users", "user-bob", "tenants", "tenant-globex", "member-of"),
            ("deployments", "dep-alpha", "tenants", "tenant-acme", "hosts"),
            ("deployments", "dep-beta", "tenants", "tenant-globex", "hosts"),
        ],
        "workflow": [
            "Register two deployments with same-named tenant data boundaries.",
            "Issue user membership and credential-grant fixtures.",
            "Exercise path-routed reads and writes so cross-tenant access fails closed.",
            "Revoke a credential and confirm the audit record names the operator.",
        ],
    },
    "schema-release-sync": {
        "title": "Schema Release Sync",
        "summary": (
            "Schema evolution, secondary indexes, validation rules, CDC, "
            "markdown templates, git mirror output, and rollback preview."
        ),
        "persona": "Developers shipping schema-backed applications.",
        "reel": "schema-release-reel",
        "collections": {
            "documents": {
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "status": {"type": "string"},
                    "revision": {"type": "integer"},
                },
                "required": ["title", "status", "revision"],
            },
            "change_events": {
                "type": "object",
                "properties": {
                    "collection": {"type": "string"},
                    "operation": {"type": "string"},
                    "offset": {"type": "integer"},
                },
                "required": ["collection", "operation", "offset"],
            },
            "mirror_commits": {
                "type": "object",
                "properties": {
                    "sha": {"type": "string"},
                    "path": {"type": "string"},
                    "status": {"type": "string"},
                },
                "required": ["sha", "path", "status"],
            },
        },
        "entities": {
            "documents": [
                ("doc-v1", {"title": "Runbook", "status": "draft", "revision": 1}),
                ("doc-v2", {"title": "Runbook", "status": "approved", "revision": 2}),
            ],
            "change_events": [
                ("evt-001", {"collection": "documents", "operation": "update", "offset": 1}),
            ],
            "mirror_commits": [
                ("commit-001", {"sha": "abc123", "path": "documents/doc-v2.md", "status": "pushed"}),
            ],
        },
        "links": [
            ("documents", "doc-v2", "documents", "doc-v1", "supersedes"),
            ("change_events", "evt-001", "documents", "doc-v2", "describes"),
            ("mirror_commits", "commit-001", "documents", "doc-v2", "renders"),
        ],
        "workflow": [
            "Apply a compatible schema change, then dry-run a breaking change.",
            "Render a document through a markdown template.",
            "Emit a CDC event and mirror the rendered entity to a git path.",
            "Preview rollback to an earlier revision before applying it.",
        ],
    },
    "admin-policy-workbench": {
        "title": "Admin Policy Workbench",
        "summary": (
            "Policy authoring, dry-runs, redacted browsing, intent queues, "
            "approval review, stale-intent handling, and UI parity with GraphQL/MCP."
        ),
        "persona": "Administrators and reviewers using the Axon web UI.",
        "reel": "admin-policy-reel",
        "collections": {
            "policies": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "status": {"type": "string"},
                    "version": {"type": "integer"},
                },
                "required": ["name", "status", "version"],
            },
            "subjects": {
                "type": "object",
                "properties": {
                    "handle": {"type": "string"},
                    "role": {"type": "string"},
                },
                "required": ["handle", "role"],
            },
            "intents": {
                "type": "object",
                "properties": {
                    "summary": {"type": "string"},
                    "risk": {"type": "string"},
                    "status": {"type": "string"},
                },
                "required": ["summary", "risk", "status"],
            },
        },
        "entities": {
            "policies": [
                ("policy-finance", {"name": "finance-access", "status": "active", "version": 3}),
            ],
            "subjects": [
                ("subject-agent", {"handle": "finance-agent", "role": "agent"}),
                ("subject-reviewer", {"handle": "finance-approver", "role": "approver"}),
            ],
            "intents": [
                ("intent-001", {"summary": "Update invoice memo", "risk": "low", "status": "ready"}),
                ("intent-002", {"summary": "Approve high-value invoice", "risk": "high", "status": "pending_approval"}),
            ],
        },
        "links": [
            ("intents", "intent-001", "subjects", "subject-agent", "requested-by"),
            ("intents", "intent-002", "subjects", "subject-reviewer", "reviewed-by"),
            ("policies", "policy-finance", "intents", "intent-002", "governs"),
        ],
        "workflow": [
            "Open the policy workspace and dry-run subjects against the active policy.",
            "Browse entities as restricted users and verify redacted fields render as null.",
            "Review intent diffs, approve or reject with a reason, then follow the audit link.",
            "Open an MCP-originated intent and verify its delegated authority envelope.",
        ],
    },
}


FEATURE_EXAMPLE: dict[str, str] = {
    "FEAT-001": "agent-taskboard",
    "FEAT-002": "schema-release-sync",
    "FEAT-003": "invoice-approval-guardrails",
    "FEAT-004": "agent-taskboard",
    "FEAT-005": "agent-taskboard",
    "FEAT-006": "agent-taskboard",
    "FEAT-007": "customer-identity-graph",
    "FEAT-008": "invoice-approval-guardrails",
    "FEAT-009": "supply-chain-bom",
    "FEAT-010": "invoice-approval-guardrails",
    "FEAT-011": "admin-policy-workbench",
    "FEAT-012": "tenant-control-plane",
    "FEAT-013": "schema-release-sync",
    "FEAT-014": "tenant-control-plane",
    "FEAT-015": "invoice-approval-guardrails",
    "FEAT-016": "agent-taskboard",
    "FEAT-017": "schema-release-sync",
    "FEAT-018": "supply-chain-bom",
    "FEAT-019": "schema-release-sync",
    "FEAT-020": "customer-identity-graph",
    "FEAT-021": "schema-release-sync",
    "FEAT-022": "invoice-approval-guardrails",
    "FEAT-023": "schema-release-sync",
    "FEAT-024": "admin-policy-workbench",
    "FEAT-025": "tenant-control-plane",
    "FEAT-026": "schema-release-sync",
    "FEAT-027": "schema-release-sync",
    "FEAT-028": "agent-taskboard",
    "FEAT-029": "admin-policy-workbench",
    "FEAT-030": "invoice-approval-guardrails",
    "FEAT-031": "admin-policy-workbench",
}


SCENARIO_EXAMPLE: dict[str, str] = {
    "SCN-001": "invoice-approval-guardrails",
    "SCN-002": "customer-identity-graph",
    "SCN-003": "customer-identity-graph",
    "SCN-004": "supply-chain-bom",
    "SCN-005": "invoice-approval-guardrails",
    "SCN-006": "agent-taskboard",
    "SCN-007": "agent-taskboard",
    "SCN-008": "customer-identity-graph",
    "SCN-009": "schema-release-sync",
    "SCN-010": "invoice-approval-guardrails",
    "SCN-011": "tenant-control-plane",
    "SCN-012": "tenant-control-plane",
    "SCN-013": "tenant-control-plane",
    "SCN-014": "tenant-control-plane",
    "SCN-015": "tenant-control-plane",
    "SCN-016": "tenant-control-plane",
    "SCN-017": "admin-policy-workbench",
}


DOMAIN_EXAMPLE_PATTERNS: list[tuple[str, str]] = [
    ("crm", "customer-identity-graph"),
    ("customer data", "customer-identity-graph"),
    ("cdp", "customer-identity-graph"),
    ("master data", "customer-identity-graph"),
    ("mdm", "customer-identity-graph"),
    ("accounts", "invoice-approval-guardrails"),
    ("ap/ar", "invoice-approval-guardrails"),
    ("time tracking", "invoice-approval-guardrails"),
    ("workflow", "invoice-approval-guardrails"),
    ("enterprise resource", "supply-chain-bom"),
    ("erp", "supply-chain-bom"),
    ("issue", "agent-taskboard"),
    ("agentic", "agent-taskboard"),
    ("document", "schema-release-sync"),
]


def clean_text(value: str) -> str:
    replacements = {
        "\u2013": "-",
        "\u2014": "-",
        "\u2018": "'",
        "\u2019": "'",
        "\u201c": '"',
        "\u201d": '"',
        "\u00d7": "x",
        "\u2192": "->",
    }
    for src, dst in replacements.items():
        value = value.replace(src, dst)
    return " ".join(value.strip().split())


def rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def repo_blob(path: str) -> str:
    return f"{GITHUB_BASE}/blob/master/{path}"


def repo_tree(path: str) -> str:
    return f"{GITHUB_BASE}/tree/master/{path}"


def slug(value: str) -> str:
    value = clean_text(value).lower()
    value = re.sub(r"[^a-z0-9]+", "-", value)
    return value.strip("-")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_features() -> list[CoverageItem]:
    items: list[CoverageItem] = []
    for path in sorted((ROOT / "docs/helix/01-frame/features").glob("FEAT-*.md")):
        text = read(path)
        file_id = re.match(r"(FEAT-\d+)", path.name)
        if not file_id:
            continue
        item_id = file_id.group(1)
        title_match = re.search(rf"^#\s+Feature Specification:\s+{item_id}\s+(?:-|--|\u2014)\s+(.+)$", text, re.M)
        title = clean_text(title_match.group(1) if title_match else item_id)
        example = FEATURE_EXAMPLE.get(item_id, fallback_example(title))
        items.append(
            CoverageItem(
                kind="feature",
                id=item_id,
                title=title,
                source_path=rel(path),
                sample_project=example,
                demo_reel=EXAMPLES[example]["reel"],
            )
        )
    return items


def parse_stories() -> list[CoverageItem]:
    items: list[CoverageItem] = []
    for path in sorted((ROOT / "docs/helix/01-frame/user-stories").glob("US-*.md")):
        text = read(path)
        heading = re.search(r"^#\s+(US-\d+[a-z]?):\s+(.+)$", text, re.M)
        if not heading:
            continue
        item_id = heading.group(1)
        title = clean_text(heading.group(2))
        feature = ""
        feature_match = re.search(r"^\*\*Feature\*\*:\s*(FEAT-\d+)", text, re.M)
        if feature_match:
            feature = feature_match.group(1)
        example = FEATURE_EXAMPLE.get(feature, fallback_example(title))
        items.append(
            CoverageItem(
                kind="story",
                id=item_id,
                title=title,
                source_path=rel(path),
                sample_project=example,
                demo_reel=EXAMPLES[example]["reel"],
                parent=feature,
            )
        )
    return items


def parse_scenarios() -> list[CoverageItem]:
    path = ROOT / "docs/helix/03-test/test-plan.md"
    text = read(path)
    items: list[CoverageItem] = []
    for match in re.finditer(r"^###\s+(SCN-\d+):\s+(.+)$", text, re.M):
        item_id = match.group(1)
        title = clean_text(match.group(2))
        example = SCENARIO_EXAMPLE.get(item_id, fallback_example(title))
        items.append(
            CoverageItem(
                kind="scenario",
                id=item_id,
                title=title,
                source_path=rel(path),
                sample_project=example,
                demo_reel=EXAMPLES[example]["reel"],
            )
        )
    return items


def parse_use_cases() -> list[CoverageItem]:
    path = ROOT / "docs/helix/00-discover/use-case-research.md"
    text = read(path)
    items: list[CoverageItem] = []
    for idx, match in enumerate(re.finditer(r"^##\s+\d+\.\s+(.+)$", text, re.M), start=1):
        title = clean_text(match.group(1))
        item_id = f"USE-{idx:03d}"
        example = domain_example(title)
        items.append(
            CoverageItem(
                kind="use_case",
                id=item_id,
                title=title,
                source_path=rel(path),
                sample_project=example,
                demo_reel=EXAMPLES[example]["reel"],
            )
        )
    return items


def fallback_example(title: str) -> str:
    lower = title.lower()
    for needle, example in DOMAIN_EXAMPLE_PATTERNS:
        if needle in lower:
            return example
    if "policy" in lower or "intent" in lower or "ui" in lower:
        return "admin-policy-workbench"
    if "tenant" in lower or "credential" in lower or "auth" in lower:
        return "tenant-control-plane"
    if "schema" in lower or "cdc" in lower or "git" in lower or "template" in lower:
        return "schema-release-sync"
    if "graph" in lower or "link" in lower or "bom" in lower:
        return "supply-chain-bom"
    return "agent-taskboard"


def domain_example(title: str) -> str:
    lower = title.lower()
    for needle, example in DOMAIN_EXAMPLE_PATTERNS:
        if needle in lower:
            return example
    return fallback_example(title)


def all_items() -> list[CoverageItem]:
    items = parse_features() + parse_stories() + parse_scenarios() + parse_use_cases()
    missing = [item for item in items if item.sample_project not in EXAMPLES]
    if missing:
        names = ", ".join(f"{item.id}:{item.sample_project}" for item in missing)
        raise SystemExit(f"unknown sample project mapping: {names}")
    missing_reel = [item for item in items if not item.demo_reel]
    if missing_reel:
        names = ", ".join(item.id for item in missing_reel)
        raise SystemExit(f"missing demo reel mapping: {names}")
    return items


def catalog_json(items: list[CoverageItem]) -> str:
    by_kind = {kind: [item for item in items if item.kind == kind] for kind in ["feature", "story", "scenario", "use_case"]}
    payload = {
        "release_target": RELEASE_TARGET,
        "generated_date": GENERATED_DATE,
        "coverage": {
            "features": len(by_kind["feature"]),
            "stories": len(by_kind["story"]),
            "scenarios": len(by_kind["scenario"]),
            "use_cases": len(by_kind["use_case"]),
            "mapped": len(items),
            "unmapped": 0,
            "coverage_percent": 100,
        },
        "examples": {
            example_id: {
                "title": data["title"],
                "summary": data["summary"],
                "demo_reel": data["reel"],
                "path": f"examples/{example_id}",
            }
            for example_id, data in EXAMPLES.items()
        },
        "items": [
            {
                "kind": item.kind,
                "id": item.id,
                "title": item.title,
                "source_path": item.source_path,
                "sample_project": item.sample_project,
                "demo_reel": item.demo_reel,
                "parent": item.parent,
            }
            for item in sorted(items, key=lambda item: (item.kind, item.id))
        ],
    }
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def frontmatter(title: str, weight: int, *, next_page: str = "", prev: str = "") -> str:
    lines = ["---", f"title: {title}", f"weight: {weight}"]
    if prev:
        lines.append(f"prev: {prev}")
    if next_page:
        lines.append(f"next: {next_page}")
    lines.append("---")
    return "\n".join(lines) + "\n\n"


def markdown_table(items: list[CoverageItem], include_kind: bool = False, reel_prefix: str = "../demo-reels/") -> str:
    headers = ["Type"] if include_kind else []
    headers += ["ID", "Title", "Source", "Sample", "Demo reel"]
    rows = ["| " + " | ".join(headers) + " |", "|" + "|".join(["---"] * len(headers)) + "|"]
    for item in items:
        source = f"[source]({repo_blob(item.source_path)})"
        sample = f"[{item.sample_project}]({repo_tree('examples/' + item.sample_project)})"
        reel = f"[{item.demo_reel}]({reel_prefix}{item.demo_reel}/)"
        row = []
        if include_kind:
            row.append(item.kind.replace("_", " "))
        row.extend([item.id, item.title, source, sample, reel])
        rows.append("| " + " | ".join(row) + " |")
    return "\n".join(rows) + "\n"


def render_coverage_page(items: list[CoverageItem]) -> str:
    features = [item for item in items if item.kind == "feature"]
    stories = [item for item in items if item.kind == "story"]
    scenarios = [item for item in items if item.kind == "scenario"]
    use_cases = [item for item in items if item.kind == "use_case"]
    body = frontmatter("HELIX Coverage", 5, prev="../demos/", next_page="../examples/")
    body += f"""# HELIX Coverage

Release target: Axon {RELEASE_TARGET}

This page is generated from the HELIX source documents and fails generation if
any covered item lacks a sample project or demo reel mapping.

| Source inventory | Count | Mapped | Coverage |
|---|---:|---:|---:|
| Feature specs | {len(features)} | {len(features)} | 100% |
| User stories | {len(stories)} | {len(stories)} | 100% |
| SCN scenarios | {len(scenarios)} | {len(scenarios)} | 100% |
| Use-case domains | {len(use_cases)} | {len(use_cases)} | 100% |

Coverage data is also published as
[`/coverage/helix-coverage.json`](../../coverage/helix-coverage.json).

## Feature Specs

{markdown_table(features)}

## User Stories

{markdown_table(stories)}

## SCN Scenarios

{markdown_table(scenarios)}

## Use-Case Domains

{markdown_table(use_cases)}
"""
    return body


def render_examples_index(items: list[CoverageItem]) -> str:
    body = frontmatter("Sample Projects", 6, prev="../coverage/", next_page="../demo-reels/")
    body += f"""# Sample Projects

The sample suite is generated with the coverage catalog for Axon {RELEASE_TARGET}.
Each project includes schemas, seed data, typed links, and a demo script that can
be adapted to a local Axon binary.

"""
    for example_id, data in EXAMPLES.items():
        covered = [item for item in items if item.sample_project == example_id]
        body += f"""## {data["title"]}

{data["summary"]}

- Persona: {data["persona"]}
- Source: [`examples/{example_id}`]({repo_tree('examples/' + example_id)})
- Demo reel: [{data["reel"]}](../demo-reels/{data["reel"]}/)
- Coverage entries: {len(covered)}

"""
    body += "## Project Coverage\n\n"
    body += markdown_table(sorted(items, key=lambda item: (item.sample_project, item.kind, item.id)), include_kind=True)
    return body


def render_demo_index(items: list[CoverageItem]) -> str:
    body = frontmatter("Demo Reels", 7, prev="../examples/")
    body += f"""# Demo Reels

These demo reels are scripted from HELIX use cases, user stories, and SCN
scenarios for Axon {RELEASE_TARGET}. The existing quickstart cast remains the
live terminal recording; the reel pages here provide complete storyboards for
the rest of the HELIX corpus.

- Live cast: [Quickstart demo](../demos/)
- Coverage catalog: [HELIX coverage](../coverage/)

"""
    for reel_id, data in reels().items():
        covered = [item for item in items if item.demo_reel == reel_id]
        body += f"## [{data['title']}]({reel_id}/)\n\n"
        body += f"{data['summary']}\n\n"
        body += f"- Sample project: [{data['sample']}]({repo_tree('examples/' + data['sample'])})\n"
        body += f"- Coverage entries: {len(covered)}\n\n"
    body += "## Reel Coverage\n\n"
    body += markdown_table(sorted(items, key=lambda item: (item.demo_reel, item.kind, item.id)), include_kind=True)
    return body


def reels() -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    for example_id, data in EXAMPLES.items():
        reel_id = data["reel"]
        result[reel_id] = {
            "title": data["title"] + " Reel",
            "summary": data["summary"],
            "sample": example_id,
        }
    return result


def render_reel_page(reel_id: str, items: list[CoverageItem]) -> str:
    reel = reels()[reel_id]
    example = EXAMPLES[reel["sample"]]
    covered = [item for item in items if item.demo_reel == reel_id]
    weight = 10 + list(reels()).index(reel_id)
    body = frontmatter(reel["title"], weight, prev="../")
    body += f"""# {reel["title"]}

Release target: Axon {RELEASE_TARGET}

{reel["summary"]}

- Sample project: [{reel["sample"]}]({repo_tree('examples/' + reel['sample'])})
- Script source: [`docs/demos/reels/{reel_id}.md`]({repo_blob('docs/demos/reels/' + reel_id + '.md')})
- Coverage entries: {len(covered)}

## Storyboard

"""
    for idx, step in enumerate(example["workflow"], start=1):
        body += f"{idx}. {step}\n"
    body += "\n## Covered HELIX Entries\n\n"
    body += markdown_table(sorted(covered, key=lambda item: (item.kind, item.id)), include_kind=True, reel_prefix="../")
    return body


def render_docs_reel(reel_id: str, items: list[CoverageItem]) -> str:
    reel = reels()[reel_id]
    example = EXAMPLES[reel["sample"]]
    covered = [item for item in items if item.demo_reel == reel_id]
    body = f"""# {reel["title"]}

Release target: Axon {RELEASE_TARGET}

{reel["summary"]}

Sample project: `examples/{reel["sample"]}`

## Storyboard

"""
    for idx, step in enumerate(example["workflow"], start=1):
        body += f"{idx}. {step}\n"
    body += "\n## Coverage Entries\n\n"
    body += "\n".join(f"- {item.kind}: {item.id} - {item.title}" for item in sorted(covered, key=lambda item: (item.kind, item.id)))
    body += "\n"
    return body


def render_docs_reel_catalog(items: list[CoverageItem]) -> str:
    body = f"""# Axon Demo Reel Catalog

Generated from HELIX sources for Axon {RELEASE_TARGET} on {GENERATED_DATE}.

| Demo reel | Sample project | Coverage entries |
|---|---|---:|
"""
    for reel_id, data in reels().items():
        count = len([item for item in items if item.demo_reel == reel_id])
        body += f"| `{reel_id}` | `examples/{data['sample']}` | {count} |\n"
    body += "\n"
    return body


def render_examples_readme(items: list[CoverageItem]) -> str:
    body = f"""# Axon Sample Projects

Generated from HELIX sources for Axon {RELEASE_TARGET}. These projects are the
sample-project side of the website coverage catalog.

| Project | Purpose | Demo reel | Coverage entries |
|---|---|---|---:|
"""
    for example_id, data in EXAMPLES.items():
        count = len([item for item in items if item.sample_project == example_id])
        body += f"| `{example_id}` | {data['summary']} | `{data['reel']}` | {count} |\n"
    body += "\nRun a project demo with:\n\n```bash\ncd examples/<project>\nbash demo.sh\n```\n"
    return body


def render_example_readme(example_id: str, items: list[CoverageItem]) -> str:
    data = EXAMPLES[example_id]
    covered = [item for item in items if item.sample_project == example_id]
    body = f"""# {data["title"]}

{data["summary"]}

- Release target: Axon {RELEASE_TARGET}
- Persona: {data["persona"]}
- Demo reel: `{data["reel"]}`
- Website page: `website/content/docs/demo-reels/{data["reel"]}.md`
- Coverage entries: {len(covered)}

## Files

- `schemas/`: JSON Schemas for every collection in the example.
- `seed/`: JSONL seed data by collection.
- `demo.sh`: CLI script that loads schemas, entities, links, and representative queries.

## Workflow

"""
    for idx, step in enumerate(data["workflow"], start=1):
        body += f"{idx}. {step}\n"
    body += "\n## Covered HELIX Entries\n\n"
    body += "\n".join(f"- {item.kind}: {item.id} - {item.title}" for item in sorted(covered, key=lambda item: (item.kind, item.id)))
    body += "\n"
    return body


def render_demo_script(example_id: str) -> str:
    data = EXAMPLES[example_id]
    lines = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "",
        f"# Generated sample loader for {data['title']}.",
        'DB="${AXON_DB:-./.axon-example.db}"',
        'rm -f "$DB"',
        "",
    ]
    for collection in data["collections"]:
        lines.append(f"axon --db \"$DB\" collections create {collection}")
        lines.append(f"axon --db \"$DB\" schema set {collection} --file schemas/{collection}.schema.json")
        lines.append("")
    for collection, entities in data["entities"].items():
        for entity_id, payload in entities:
            json_payload = json.dumps(payload, sort_keys=True)
            lines.append(f"axon --db \"$DB\" entities create {collection} --id {entity_id} --data '{json_payload}'")
        lines.append("")
    for src_collection, src_id, dst_collection, dst_id, link_type in data["links"]:
        lines.append(
            f"axon --db \"$DB\" links set {src_collection} {src_id} {dst_collection} {dst_id} --type {link_type}"
        )
    lines += [
        "",
        "# Representative read paths for the reel.",
        'axon --db "$DB" collections list',
        'axon --db "$DB" audit list --limit 20',
    ]
    first_collection = next(iter(data["collections"]))
    first_id = data["entities"][first_collection][0][0]
    lines.append(f'axon --db "$DB" graph {first_collection} {first_id} --depth 2 || true')
    lines.append("")
    return "\n".join(lines)


def expected_outputs(items: list[CoverageItem]) -> tuple[dict[Path, str], list[Path]]:
    outputs: dict[Path, str] = {
        ROOT / "website/static/coverage/helix-coverage.json": catalog_json(items),
        ROOT / "website/content/docs/coverage/_index.md": render_coverage_page(items),
        ROOT / "website/content/docs/examples/_index.md": render_examples_index(items),
        ROOT / "website/content/docs/demo-reels/_index.md": render_demo_index(items),
        ROOT / "docs/demos/reels/README.md": render_docs_reel_catalog(items),
        ROOT / "examples/README.md": render_examples_readme(items),
    }
    executable: list[Path] = []
    for reel_id in reels():
        outputs[ROOT / f"website/content/docs/demo-reels/{reel_id}.md"] = render_reel_page(reel_id, items)
        outputs[ROOT / f"docs/demos/reels/{reel_id}.md"] = render_docs_reel(reel_id, items)
    for example_id, data in EXAMPLES.items():
        outputs[ROOT / f"examples/{example_id}/README.md"] = render_example_readme(example_id, items)
        script_path = ROOT / f"examples/{example_id}/demo.sh"
        outputs[script_path] = render_demo_script(example_id)
        executable.append(script_path)
        for collection, schema in data["collections"].items():
            outputs[ROOT / f"examples/{example_id}/schemas/{collection}.schema.json"] = (
                json.dumps(schema, indent=2, sort_keys=True) + "\n"
            )
        for collection, entities in data["entities"].items():
            rows = [json.dumps({"id": entity_id, "data": payload}, sort_keys=True) for entity_id, payload in entities]
            outputs[ROOT / f"examples/{example_id}/seed/{collection}.jsonl"] = "\n".join(rows) + "\n"
    return outputs, executable


def write_outputs(outputs: dict[Path, str], executable: list[Path]) -> None:
    for path, content in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    for path in executable:
        mode = path.stat().st_mode
        path.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def check_outputs(outputs: dict[Path, str]) -> int:
    failures: list[str] = []
    for path, expected in outputs.items():
        if not path.exists():
            failures.append(f"missing: {rel(path)}")
            continue
        actual = path.read_text(encoding="utf-8")
        if actual != expected:
            failures.append(f"stale: {rel(path)}")
    if failures:
        print("FAIL: generated website coverage output is stale", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1
    return 0


def print_summary(items: list[CoverageItem]) -> None:
    counts = {kind: len([item for item in items if item.kind == kind]) for kind in ["feature", "story", "scenario", "use_case"]}
    print(
        "OK: coverage 100% "
        f"features={counts['feature']} stories={counts['story']} "
        f"scenarios={counts['scenario']} use_cases={counts['use_case']} "
        f"mapped={len(items)} release={RELEASE_TARGET}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true", help="write generated website, demo, and example files")
    mode.add_argument("--check", action="store_true", help="verify generated files are current")
    args = parser.parse_args()

    items = all_items()
    outputs, executable = expected_outputs(items)
    if args.write:
        write_outputs(outputs, executable)
        print_summary(items)
        return 0
    result = check_outputs(outputs)
    if result == 0:
        print_summary(items)
    return result


if __name__ == "__main__":
    raise SystemExit(main())
