#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Customer Identity Graph.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create contacts
axon --db "$DB" schema set contacts --file schemas/contacts.schema.json

axon --db "$DB" collections create companies
axon --db "$DB" schema set companies --file schemas/companies.schema.json

axon --db "$DB" collections create profiles
axon --db "$DB" schema set profiles --file schemas/profiles.schema.json

axon --db "$DB" entities create contacts --id contact-a --data '{"email": "jane@example.com", "name": "Jane Doe", "source": "crm"}'
axon --db "$DB" entities create contacts --id contact-b --data '{"email": "jane@example.com", "name": "J. Doe", "source": "support"}'

axon --db "$DB" entities create companies --id company-acme --data '{"name": "Acme Corp", "tier": "enterprise"}'

axon --db "$DB" entities create profiles --id profile-jane --data '{"canonical_email": "jane@example.com", "confidence": 0.96, "segment": "enterprise"}'

axon --db "$DB" links set contacts contact-a companies company-acme --type works-at
axon --db "$DB" links set contacts contact-b companies company-acme --type works-at
axon --db "$DB" links set profiles profile-jane contacts contact-a --type resolved-from
axon --db "$DB" links set profiles profile-jane contacts contact-b --type resolved-from

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph contacts contact-a --depth 2 || true
