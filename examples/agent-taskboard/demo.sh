#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Agent Taskboard.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create projects
axon --db "$DB" schema set projects --file schemas/projects.schema.json

axon --db "$DB" collections create beads
axon --db "$DB" schema set beads --file schemas/beads.schema.json

axon --db "$DB" collections create agents
axon --db "$DB" schema set agents --file schemas/agents.schema.json

axon --db "$DB" entities create projects --id axon-core --data '{"name": "Axon core", "owner": "platform", "phase": "release-0.7.1"}'

axon --db "$DB" entities create agents --id agent-alpha --data '{"name": "Agent Alpha", "role": "builder", "scope": "storage"}'
axon --db "$DB" entities create agents --id agent-beta --data '{"name": "Agent Beta", "role": "reviewer", "scope": "policy"}'

axon --db "$DB" entities create beads --id bead-001 --data '{"agent": "agent-alpha", "priority": 1, "status": "done", "title": "Define schema"}'
axon --db "$DB" entities create beads --id bead-002 --data '{"agent": "agent-beta", "priority": 2, "status": "ready", "title": "Wire graph query"}'
axon --db "$DB" entities create beads --id bead-003 --data '{"agent": "agent-alpha", "priority": 3, "status": "ready", "title": "Record audit demo"}'

axon --db "$DB" links set beads bead-002 beads bead-001 --type depends-on
axon --db "$DB" links set beads bead-003 projects axon-core --type belongs-to
axon --db "$DB" links set beads bead-002 agents agent-beta --type assigned-to

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph projects axon-core --depth 2 || true
