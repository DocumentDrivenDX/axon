#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Invoice Approval Guardrails.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create vendors
axon --db "$DB" schema set vendors --file schemas/vendors.schema.json

axon --db "$DB" collections create invoices
axon --db "$DB" schema set invoices --file schemas/invoices.schema.json

axon --db "$DB" collections create payments
axon --db "$DB" schema set payments --file schemas/payments.schema.json

axon --db "$DB" entities create vendors --id vendor-acme --data '{"name": "Acme Supplies", "risk": "medium"}'

axon --db "$DB" entities create invoices --id inv-030 --data '{"amount": 5000, "number": "INV-030", "requires_approval": true, "status": "submitted"}'
axon --db "$DB" entities create invoices --id inv-035 --data '{"amount": 5000, "number": "INV-035", "requires_approval": true, "status": "submitted"}'

axon --db "$DB" entities create payments --id pmt-107 --data '{"amount": 7500, "reference": "PMT-107", "status": "received"}'

axon --db "$DB" links set invoices inv-030 vendors vendor-acme --type billed-by
axon --db "$DB" links set invoices inv-035 vendors vendor-acme --type billed-by
axon --db "$DB" links set payments pmt-107 invoices inv-030 --type applies-to
axon --db "$DB" links set payments pmt-107 invoices inv-035 --type applies-to

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph vendors vendor-acme --depth 2 || true
