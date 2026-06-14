#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Tenant Control Plane.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create tenants
axon --db "$DB" schema set tenants --file schemas/tenants.schema.json

axon --db "$DB" collections create users
axon --db "$DB" schema set users --file schemas/users.schema.json

axon --db "$DB" collections create deployments
axon --db "$DB" schema set deployments --file schemas/deployments.schema.json

axon --db "$DB" entities create tenants --id tenant-acme --data '{"name": "acme", "plan": "enterprise"}'
axon --db "$DB" entities create tenants --id tenant-globex --data '{"name": "globex", "plan": "team"}'

axon --db "$DB" entities create users --id user-alice --data '{"email": "alice@example.com", "role": "admin"}'
axon --db "$DB" entities create users --id user-bob --data '{"email": "bob@example.com", "role": "read"}'

axon --db "$DB" entities create deployments --id dep-alpha --data '{"name": "dep-alpha", "region": "local-a", "status": "registered"}'
axon --db "$DB" entities create deployments --id dep-beta --data '{"name": "dep-beta", "region": "local-b", "status": "registered"}'

axon --db "$DB" links set users user-alice tenants tenant-acme --type member-of
axon --db "$DB" links set users user-bob tenants tenant-globex --type member-of
axon --db "$DB" links set deployments dep-alpha tenants tenant-acme --type hosts
axon --db "$DB" links set deployments dep-beta tenants tenant-globex --type hosts

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph tenants tenant-acme --depth 2 || true
