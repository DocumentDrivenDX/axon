#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Schema Release Sync.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create documents
axon --db "$DB" schema set documents --file schemas/documents.schema.json

axon --db "$DB" collections create change_events
axon --db "$DB" schema set change_events --file schemas/change_events.schema.json

axon --db "$DB" collections create mirror_commits
axon --db "$DB" schema set mirror_commits --file schemas/mirror_commits.schema.json

axon --db "$DB" entities create documents --id doc-v1 --data '{"revision": 1, "status": "draft", "title": "Runbook"}'
axon --db "$DB" entities create documents --id doc-v2 --data '{"revision": 2, "status": "approved", "title": "Runbook"}'

axon --db "$DB" entities create change_events --id evt-001 --data '{"collection": "documents", "offset": 1, "operation": "update"}'

axon --db "$DB" entities create mirror_commits --id commit-001 --data '{"path": "documents/doc-v2.md", "sha": "abc123", "status": "pushed"}'

axon --db "$DB" links set documents doc-v2 documents doc-v1 --type supersedes
axon --db "$DB" links set change_events evt-001 documents doc-v2 --type describes
axon --db "$DB" links set mirror_commits commit-001 documents doc-v2 --type renders

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph documents doc-v1 --depth 2 || true
