#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Admin Policy Workbench.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create policies
axon --db "$DB" schema set policies --file schemas/policies.schema.json

axon --db "$DB" collections create subjects
axon --db "$DB" schema set subjects --file schemas/subjects.schema.json

axon --db "$DB" collections create intents
axon --db "$DB" schema set intents --file schemas/intents.schema.json

axon --db "$DB" entities create policies --id policy-finance --data '{"name": "finance-access", "status": "active", "version": 3}'

axon --db "$DB" entities create subjects --id subject-agent --data '{"handle": "finance-agent", "role": "agent"}'
axon --db "$DB" entities create subjects --id subject-reviewer --data '{"handle": "finance-approver", "role": "approver"}'

axon --db "$DB" entities create intents --id intent-001 --data '{"risk": "low", "status": "ready", "summary": "Update invoice memo"}'
axon --db "$DB" entities create intents --id intent-002 --data '{"risk": "high", "status": "pending_approval", "summary": "Approve high-value invoice"}'

axon --db "$DB" links set intents intent-001 subjects subject-agent --type requested-by
axon --db "$DB" links set intents intent-002 subjects subject-reviewer --type reviewed-by
axon --db "$DB" links set policies policy-finance intents intent-002 --type governs

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph policies policy-finance --depth 2 || true
