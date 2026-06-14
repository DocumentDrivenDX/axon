#!/usr/bin/env bash
set -euo pipefail

# Generated sample loader for Supply Chain BOM.
DB="${AXON_DB:-./.axon-example.db}"
rm -f "$DB"

axon --db "$DB" collections create products
axon --db "$DB" schema set products --file schemas/products.schema.json

axon --db "$DB" collections create build_orders
axon --db "$DB" schema set build_orders --file schemas/build_orders.schema.json

axon --db "$DB" entities create products --id widget-a --data '{"kind": "finished-good", "name": "Widget A", "sku": "WIDGET-A"}'
axon --db "$DB" entities create products --id sub-b --data '{"kind": "assembly", "name": "Sub Assembly B", "sku": "SUB-B"}'
axon --db "$DB" entities create products --id component-c --data '{"kind": "part", "name": "Component C", "sku": "COMP-C"}'
axon --db "$DB" entities create products --id component-d --data '{"kind": "part", "name": "Component D", "sku": "COMP-D"}'

axon --db "$DB" entities create build_orders --id bo-100 --data '{"order_number": "BO-100", "quantity": 25, "status": "planned"}'

axon --db "$DB" links set products widget-a products sub-b --type contains
axon --db "$DB" links set products widget-a products component-c --type contains
axon --db "$DB" links set products sub-b products component-c --type contains
axon --db "$DB" links set products sub-b products component-d --type contains
axon --db "$DB" links set build_orders bo-100 products widget-a --type builds

# Representative read paths for the reel.
axon --db "$DB" collections list
axon --db "$DB" audit list --limit 20
axon --db "$DB" graph products widget-a --depth 2 || true
