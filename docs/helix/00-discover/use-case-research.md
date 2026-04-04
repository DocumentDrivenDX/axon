# Use Case Research: Axon Domain Applications

**Date**: 2026-04-04
**Author**: Erik LaBianca
**Status**: Draft
**Purpose**: Drive feature specification and user story creation by grounding Axon's entity-graph-relational model in concrete domain use cases.

---

## Table of Contents

1. [CRM (Customer Relationship Management)](#1-crm-customer-relationship-management)
2. [CDP (Customer Data Platform)](#2-cdp-customer-data-platform)
3. [AP/AR (Accounts Payable / Accounts Receivable)](#3-apar-accounts-payable--accounts-receivable)
4. [Time Tracking](#4-time-tracking)
5. [ERP (Enterprise Resource Planning)](#5-erp-enterprise-resource-planning)
6. [Issue Tracking](#6-issue-tracking)
7. [Document Management](#7-document-management)
8. [MDM (Master Data Management)](#8-mdm-master-data-management)
9. [Workflow Automation](#9-workflow-automation)
10. [Agentic Applications](#10-agentic-applications)

---

## 1. CRM (Customer Relationship Management)

### Sample Entity Schemas

#### Contact

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `first_name` | `string` | Required |
| `last_name` | `string` | Required |
| `email` | `string` | Unique within collection |
| `phone_numbers` | `array<object>` | `[{ type: "work"\|"mobile"\|"home", number: string, primary: bool }]` |
| `address` | `object` | `{ street: string, city: string, state: string, postal_code: string, country: string }` |
| `title` | `string` | Job title |
| `source` | `string` | `"inbound"`, `"outbound"`, `"referral"`, `"import"` |
| `tags` | `array<string>` | Freeform tags |
| `custom_fields` | `object` | Tenant-defined key-value pairs |

#### Company

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | Required |
| `domain` | `string` | Primary web domain |
| `industry` | `string` | `"saas"`, `"finance"`, `"healthcare"`, etc. |
| `employee_count` | `int` | Approximate headcount |
| `annual_revenue` | `object` | `{ amount: decimal, currency: string }` |
| `address` | `object` | `{ street, city, state, postal_code, country }` |
| `tier` | `string` | `"enterprise"`, `"mid-market"`, `"smb"` |

#### Deal

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `title` | `string` | Required |
| `stage` | `string` | `"prospecting"`, `"qualification"`, `"proposal"`, `"negotiation"`, `"closed_won"`, `"closed_lost"` |
| `amount` | `object` | `{ value: decimal, currency: string }` |
| `probability` | `float` | 0.0-1.0 |
| `expected_close_date` | `date` | |
| `actual_close_date` | `date\|null` | Set when stage reaches `closed_*` |
| `loss_reason` | `string\|null` | Set when `closed_lost` |
| `pipeline_id` | `string` | Reference to pipeline config |
| `notes` | `string` | Freeform |

#### Activity

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `type` | `string` | `"call"`, `"email"`, `"meeting"`, `"note"`, `"task"` |
| `subject` | `string` | Required |
| `body` | `string` | Detail/content |
| `occurred_at` | `datetime` | When activity happened |
| `duration_minutes` | `int\|null` | For calls/meetings |
| `outcome` | `string\|null` | `"connected"`, `"voicemail"`, `"no_answer"`, `"completed"` |
| `completed` | `bool` | For tasks |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `works-at` | Contact | Company | `{ role: string, since: date, primary: bool }` | Contact-company employment |
| `owns-deal` | Contact | Deal | `{ role: "decision_maker"\|"champion"\|"influencer" }` | Deal stakeholder |
| `related-to-deal` | Company | Deal | `{}` | Company associated with deal |
| `logged-activity` | Contact | Activity | `{}` | Activity performed by/with contact |
| `spawned-from` | Deal | Deal | `{ reason: string }` | Upsell/renewal lineage |

### Key Workflows

**1. Deal Stage Progression**

A sales rep advances a deal through the pipeline:

1. `BEGIN TRANSACTION`
2. `UPDATE deals/deal-123 SET stage = "proposal", probability = 0.6 WHERE _version = 3`
3. `CREATE activities { type: "note", subject: "Moved to proposal", body: "Sent SOW v2" }`
4. `CREATE LINK logged-activity FROM contacts/rep-456 TO activities/act-new`
5. `COMMIT`

Audit captures: who moved the deal, when, from which stage, and the associated activity. The entire state change is one atomic unit.

**2. Contact Merge (Duplicate Resolution)**

Two contact records represent the same person:

1. `BEGIN TRANSACTION`
2. Read all links from `contacts/contact-A` (the loser)
3. Re-create each link pointing to `contacts/contact-B` (the winner) with preserved metadata
4. `UPDATE contacts/contact-B` merging fields from A (e.g., additional phone numbers)
5. `DELETE contacts/contact-A`
6. `COMMIT`

Atomicity ensures no orphaned links. Audit log preserves the full merge history for undo.

**3. Pipeline Reporting Query**

Forecast revenue by stage:

```
SELECT stage, SUM(amount.value), COUNT(*)
FROM deals
WHERE expected_close_date BETWEEN '2026-04-01' AND '2026-06-30'
GROUP BY stage
```

Graph traversal augments this: "Show me all deals where the primary contact works at a company in tier = enterprise."

### Why Axon Fits

- **Links**: Contact-company-deal relationships are the core of CRM. Typed links (`works-at`, `owns-deal`) replace fragile foreign-key join tables. Querying "all deals where a contact who works-at a company in healthcare is a decision-maker" is a natural link traversal, not a 4-way SQL JOIN
- **Audit**: Sales management demands knowing who changed a deal stage and when. Axon's audit log provides this without bolt-on solutions. Compliance requirements (SOX, revenue recognition) need immutable mutation history
- **Transactions**: Contact merge requires atomically moving links and updating entities. Without ACID, merges produce orphaned links and inconsistent state
- **Schema**: Contact and deal schemas evolve frequently (custom fields, new stages). Schema-first validation prevents agents and integrations from writing garbage data

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Contacts | 10K-500K per instance |
| Companies | 1K-50K |
| Deals | 5K-100K active, millions historical |
| Activities | 100K-10M |
| Read:Write ratio | 10:1 (heavy reads for dashboards, search) |
| Query patterns | Filter by stage/owner/date, aggregate by pipeline, traverse contact-company-deal graph |

---

## 2. CDP (Customer Data Platform)

### Sample Entity Schemas

#### Unified Profile

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `canonical_email` | `string\|null` | Best-known email |
| `canonical_phone` | `string\|null` | Best-known phone |
| `identifiers` | `array<object>` | `[{ source: string, type: "email"\|"phone"\|"device_id"\|"crm_id", value: string, confidence: float }]` |
| `demographics` | `object` | `{ first_name, last_name, age_range, gender, city, state, country }` |
| `behavioral_summary` | `object` | `{ total_events: int, first_seen: datetime, last_seen: datetime, last_channel: string }` |
| `segments` | `array<string>` | Active segment memberships: `["high-value", "churning", "newsletter-subscriber"]` |
| `consent` | `object` | `{ email_opt_in: bool, sms_opt_in: bool, updated_at: datetime, source: string }` |
| `merge_history` | `array<object>` | `[{ merged_profile_id: string, merged_at: datetime, actor: string }]` |

#### Source Record

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `source_system` | `string` | `"salesforce"`, `"stripe"`, `"segment"`, `"snowplow"` |
| `source_id` | `string` | ID in the source system |
| `raw_payload` | `object` | Original record as received |
| `extracted_identifiers` | `array<object>` | `[{ type: string, value: string }]` |
| `ingested_at` | `datetime` | When Axon received this record |
| `matched` | `bool` | Whether identity resolution has processed this record |

#### Event

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `event_type` | `string` | `"page_view"`, `"purchase"`, `"email_open"`, `"support_ticket"`, `"app_login"` |
| `occurred_at` | `datetime` | When the event occurred in the real world |
| `channel` | `string` | `"web"`, `"mobile"`, `"email"`, `"in-store"`, `"api"` |
| `properties` | `object` | Event-type-specific: `{ url, product_id, amount, subject, ... }` |
| `context` | `object` | `{ ip: string, user_agent: string, device_id: string, session_id: string }` |

#### Segment

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | `"high-value"`, `"at-risk"`, `"newsletter-subscriber"` |
| `description` | `string` | Human-readable definition |
| `criteria` | `object` | `{ rules: [{ field: string, op: string, value: any }], combinator: "and"\|"or" }` |
| `member_count` | `int` | Cached count, updated on recomputation |
| `last_computed_at` | `datetime` | |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `resolved-from` | Unified Profile | Source Record | `{ confidence: float, match_rule: string, resolved_at: datetime }` | Identity resolution lineage |
| `performed` | Unified Profile | Event | `{}` | Profile-event attribution |
| `member-of` | Unified Profile | Segment | `{ joined_at: datetime, qualifying_event_id: string\|null }` | Segment membership |
| `supersedes` | Unified Profile | Unified Profile | `{ merged_at: datetime, reason: string }` | Profile merge history |

### Key Workflows

**1. Identity Resolution (Source Record Ingestion)**

A new source record arrives from Salesforce:

1. `CREATE source-records { source_system: "salesforce", source_id: "003xx...", raw_payload: {...}, extracted_identifiers: [{type: "email", value: "jane@acme.com"}] }`
2. Query `unified-profiles` for existing profiles matching `identifiers[].value = "jane@acme.com"`
3. If match found with confidence >= 0.9:
   - `BEGIN TRANSACTION`
   - `UPDATE unified-profiles/profile-789` — merge new identifiers, update demographics if newer
   - `CREATE LINK resolved-from FROM unified-profiles/profile-789 TO source-records/sr-new { confidence: 0.95, match_rule: "exact-email" }`
   - `UPDATE source-records/sr-new SET matched = true`
   - `COMMIT`
4. If no match: create new unified profile and link

Audit trail records every identity resolution decision — critical for GDPR "right to explanation."

**2. Segment Recomputation**

Nightly job recomputes the "high-value" segment:

1. Query events: `SELECT profile_id FROM events WHERE event_type = "purchase" AND properties.amount > 500 AND occurred_at > now() - 90 days`
2. `BEGIN TRANSACTION`
3. For each qualifying profile not already in segment: `CREATE LINK member-of FROM profile TO segment { joined_at: now() }`
4. For each profile in segment that no longer qualifies: `DELETE LINK member-of FROM profile TO segment`
5. `UPDATE segments/seg-high-value SET member_count = <new_count>, last_computed_at = now()`
6. `COMMIT`

Transactional guarantees ensure segment membership is always consistent with the member count.

**3. Profile Merge**

Two profiles are discovered to represent the same person:

1. `BEGIN TRANSACTION`
2. Re-point all `performed` links from loser profile to winner profile
3. Re-point all `member-of` links (deduplicating where both profiles were in the same segment)
4. Re-point all `resolved-from` links
5. `CREATE LINK supersedes FROM winner TO loser { merged_at: now(), reason: "manual-review" }`
6. `UPDATE winner` — merge identifiers, demographics, behavioral_summary
7. `DELETE loser`
8. `COMMIT`

### Why Axon Fits

- **Links with metadata**: Identity resolution is fundamentally about links — which source records resolve to which profiles, at what confidence, by which rule. Axon's typed links with metadata (`confidence`, `match_rule`) make resolution lineage a first-class queryable structure, not a side table
- **Audit**: GDPR and CCPA require explaining why a customer's data was unified in a particular way. The audit log provides a complete, immutable record of every resolution decision, merge, and segment change
- **Transactions**: Profile merges involve moving dozens of links atomically. Without ACID, merges leave dangling links and inconsistent segment counts
- **Schema**: Source records arrive in varied formats. Schema validation on unified profiles ensures that no matter what garbage arrives from upstream, the golden profile always conforms to a known structure

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Unified Profiles | 100K-10M |
| Source Records | 500K-50M |
| Events | 1M-100M (often partitioned by time) |
| Segments | 50-500 |
| Read:Write ratio | 3:1 (heavy writes from event streams, heavy reads from segment queries and activation) |
| Query patterns | Identity lookup by email/phone/device, segment membership queries, event stream filtering by profile + time range |

---

## 3. AP/AR (Accounts Payable / Accounts Receivable)

### Sample Entity Schemas

#### Invoice

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `invoice_number` | `string` | Human-readable, unique (e.g., `"INV-2026-0042"`) |
| `direction` | `string` | `"payable"` or `"receivable"` |
| `status` | `string` | `"draft"`, `"submitted"`, `"approved"`, `"paid"`, `"partially_paid"`, `"overdue"`, `"voided"` |
| `issued_date` | `date` | |
| `due_date` | `date` | |
| `line_items` | `array<object>` | `[{ description: string, quantity: decimal, unit_price: decimal, tax_rate: decimal, amount: decimal, gl_account: string }]` |
| `subtotal` | `decimal` | Sum of line item amounts |
| `tax_total` | `decimal` | Sum of line item taxes |
| `total` | `decimal` | subtotal + tax_total |
| `currency` | `string` | ISO 4217: `"USD"`, `"EUR"` |
| `payment_terms` | `string` | `"net-30"`, `"net-60"`, `"due-on-receipt"` |
| `notes` | `string` | |

#### Payment

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `payment_number` | `string` | `"PMT-2026-0107"` |
| `amount` | `decimal` | Payment amount |
| `currency` | `string` | ISO 4217 |
| `method` | `string` | `"ach"`, `"wire"`, `"check"`, `"credit_card"`, `"cash"` |
| `reference` | `string` | Check number, wire reference, transaction ID |
| `payment_date` | `date` | |
| `status` | `string` | `"pending"`, `"cleared"`, `"bounced"`, `"refunded"` |

#### Ledger Entry

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `entry_date` | `date` | |
| `gl_account` | `string` | `"1200-accounts-receivable"`, `"2000-accounts-payable"`, `"4000-revenue"` |
| `debit` | `decimal` | |
| `credit` | `decimal` | |
| `memo` | `string` | |
| `posting_period` | `string` | `"2026-04"` |
| `posted` | `bool` | Immutable once true |

#### Vendor / Customer

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | Required |
| `type` | `string` | `"vendor"` or `"customer"` |
| `tax_id` | `string\|null` | EIN, VAT number |
| `payment_terms_default` | `string` | Default terms for new invoices |
| `bank_details` | `object` | `{ bank_name, routing_number, account_number, swift_code }` |
| `address` | `object` | `{ street, city, state, postal_code, country }` |
| `balance` | `decimal` | Current outstanding balance |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `billed-to` | Invoice | Customer/Vendor | `{}` | Who the invoice is for/from |
| `paid-by` | Payment | Invoice | `{ amount_applied: decimal }` | Partial or full payment application |
| `posted-as` | Invoice | Ledger Entry | `{}` | Invoice-to-GL posting |
| `approved-by` | Invoice | Contact | `{ approved_at: datetime, level: int }` | Approval chain record |

### Key Workflows

**1. Invoice Approval and Posting**

An AP invoice arrives and flows through approval:

1. `CREATE invoices { invoice_number: "INV-2026-0042", direction: "payable", status: "submitted", ... }`
2. `CREATE LINK billed-to FROM invoices/inv-042 TO vendors/vendor-acme`
3. Approver reviews and approves:
   - `BEGIN TRANSACTION`
   - `UPDATE invoices/inv-042 SET status = "approved" WHERE _version = 1`
   - `CREATE LINK approved-by FROM invoices/inv-042 TO contacts/approver-jane { approved_at: "2026-04-04T14:30:00Z", level: 1 }`
   - `CREATE ledger-entries { gl_account: "2000-accounts-payable", credit: 5000.00, ... }`
   - `CREATE ledger-entries { gl_account: "6100-office-supplies", debit: 5000.00, ... }`
   - `CREATE LINK posted-as FROM invoices/inv-042 TO ledger-entries/le-new-1`
   - `CREATE LINK posted-as FROM invoices/inv-042 TO ledger-entries/le-new-2`
   - `COMMIT`

Every financial state change is one atomic transaction. The audit log produces a complete approval trail for auditors.

**2. Payment Application with Partial Payment**

A customer payment arrives that covers part of two invoices:

1. `BEGIN TRANSACTION`
2. `CREATE payments { payment_number: "PMT-2026-0107", amount: 7500.00, method: "ach", status: "cleared" }`
3. `CREATE LINK paid-by FROM payments/pmt-107 TO invoices/inv-030 { amount_applied: 5000.00 }`
4. `CREATE LINK paid-by FROM payments/pmt-107 TO invoices/inv-035 { amount_applied: 2500.00 }`
5. `UPDATE invoices/inv-030 SET status = "paid" WHERE _version = 3`
6. `UPDATE invoices/inv-035 SET status = "partially_paid" WHERE _version = 2`
7. `CREATE ledger-entries { gl_account: "1000-cash", debit: 7500.00, ... }`
8. `CREATE ledger-entries { gl_account: "1200-accounts-receivable", credit: 7500.00, ... }`
9. `UPDATE customers/cust-456 SET balance = balance - 7500.00 WHERE _version = 12`
10. `COMMIT`

If any step fails (version conflict on customer balance, schema violation), the entire payment application rolls back. No half-applied payments.

**3. Month-End Reconciliation**

Reconcile ledger entries against bank statements:

1. Query: `SELECT gl_account, SUM(debit), SUM(credit) FROM ledger-entries WHERE posting_period = "2026-03" GROUP BY gl_account`
2. Compare against bank feed totals
3. For unmatched items, create reconciliation adjustment entries in a new transaction
4. Audit log provides a complete trail for external auditors — every ledger entry traces back to an invoice, payment, or adjustment with actor and timestamp

### Why Axon Fits

- **Transactions**: AP/AR is the canonical case for ACID. Debiting one account and crediting another must be atomic. Payment application across multiple invoices must be all-or-nothing. Axon's cross-collection transactions handle this natively
- **Audit**: Financial systems require immutable audit trails for SOX compliance, external audits, and tax reporting. Axon's audit log — with before/after state, actor, and timestamp on every mutation — is exactly what auditors need. No bolt-on audit tables
- **Links with metadata**: `paid-by` links carry `amount_applied` — essential for partial payments. `approved-by` links carry approval level and timestamp. These are first-class queryable relationships, not comment fields
- **Schema**: Financial data demands strict schemas. An invoice without a `total` or a ledger entry without a `gl_account` is not just wrong, it is compliance-violating. Schema-first validation prevents garbage from entering the system

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Invoices | 10K-500K per year |
| Payments | 5K-200K per year |
| Ledger Entries | 50K-2M per year |
| Vendors/Customers | 500-50K |
| Read:Write ratio | 5:1 (batch writes during payment runs, heavy reads for reporting and reconciliation) |
| Query patterns | Filter by status/date/vendor, aggregate by GL account and period, traverse invoice-payment-ledger chain |

---

## 4. Time Tracking

### Sample Entity Schemas

#### Project

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | Required |
| `code` | `string` | Short code: `"AXON"`, `"WEBSITE-REDESIGN"` |
| `client_name` | `string` | |
| `status` | `string` | `"active"`, `"on-hold"`, `"completed"`, `"archived"` |
| `budget` | `object` | `{ hours: decimal, amount: decimal, currency: string }` |
| `billing_type` | `string` | `"time-and-materials"`, `"fixed-price"`, `"internal"` |
| `start_date` | `date` | |
| `end_date` | `date\|null` | |

#### Task

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | Required |
| `description` | `string` | |
| `status` | `string` | `"open"`, `"in_progress"`, `"completed"` |
| `estimated_hours` | `decimal\|null` | |
| `billable` | `bool` | Default true |

#### Time Entry

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `date` | `date` | Work date |
| `hours` | `decimal` | Duration in hours (e.g., `2.5`) |
| `description` | `string` | What was done |
| `billable` | `bool` | |
| `billing_rate` | `decimal\|null` | Rate at time of entry (snapshot) |
| `approval_status` | `string` | `"pending"`, `"approved"`, `"rejected"` |
| `rejection_reason` | `string\|null` | |
| `locked` | `bool` | Immutable after invoicing |
| `timer` | `object\|null` | `{ started_at: datetime, running: bool }` for live timers |

#### Billing Rate

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `effective_date` | `date` | When this rate takes effect |
| `rate` | `decimal` | Hourly rate |
| `currency` | `string` | ISO 4217 |
| `type` | `string` | `"standard"`, `"overtime"`, `"weekend"` |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `belongs-to-project` | Task | Project | `{}` | Task-project membership |
| `logged-against` | Time Entry | Task | `{}` | What task the time is for |
| `logged-by` | Time Entry | (external user ref) | `{}` | Who logged the time |
| `approved-by` | Time Entry | (external user ref) | `{ approved_at: datetime }` | Approval record |
| `billed-on` | Time Entry | Invoice | `{ line_item_index: int }` | Links time to generated invoice |

### Key Workflows

**1. Weekly Timesheet Submission and Approval**

An employee submits their weekly timesheet:

1. Employee creates time entries throughout the week (individual creates, no transaction needed)
2. On Friday, submit for approval:
   - `BEGIN TRANSACTION`
   - For each time entry in the week: `UPDATE time-entries/te-xxx SET approval_status = "pending" WHERE _version = N`
   - Schema validation ensures `hours > 0`, `date` is within the submission period, `billable` flag is set
   - `COMMIT`
3. Manager approves:
   - `BEGIN TRANSACTION`
   - For each entry: `UPDATE time-entries/te-xxx SET approval_status = "approved" WHERE _version = M`
   - `CREATE LINK approved-by FROM time-entries/te-xxx TO users/manager-id { approved_at: now() }`
   - `COMMIT`

Audit log captures exactly who approved which entries and when — required for billing disputes.

**2. Invoice Generation from Approved Time**

Generate a client invoice from approved billable time:

1. Query: `SELECT * FROM time-entries WHERE approval_status = "approved" AND billable = true AND locked = false` filtered by project links to the target client
2. `BEGIN TRANSACTION`
3. `CREATE invoices { invoice_number: "INV-...", direction: "receivable", line_items: [computed from time entries], total: sum, ... }`
4. For each time entry included: `UPDATE time-entries/te-xxx SET locked = true WHERE _version = N`
5. For each time entry: `CREATE LINK billed-on FROM time-entries/te-xxx TO invoices/inv-new { line_item_index: i }`
6. `COMMIT`

Locking time entries in the same transaction as invoice creation ensures no time entry is double-billed.

**3. Budget Burn-Down Query**

Track project budget consumption:

1. Traverse `belongs-to-project` links to find all tasks for project X
2. Traverse `logged-against` links to find all time entries for those tasks
3. Aggregate: `SUM(hours) WHERE billable = true` and `SUM(hours * billing_rate) WHERE billable = true`
4. Compare against `projects/proj-X.budget.hours` and `projects/proj-X.budget.amount`

This is a natural graph traversal + aggregation: project -> tasks -> time entries, filtered and summed.

### Why Axon Fits

- **Audit**: Timesheet approvals, modifications, and billing are compliance-sensitive. Clients dispute invoices; the audit trail shows the exact approval chain. An employee edits a time entry after submission; audit shows the before/after
- **Transactions**: Invoice generation must atomically lock time entries and create the invoice. Without this, race conditions produce double-billing
- **Links**: The project -> task -> time entry -> invoice chain is a natural graph. "Show me all unbilled time for client X" requires traversing this chain. `billed-on` links with `line_item_index` metadata create a precise audit trail from invoice line items back to individual time entries
- **Schema**: Time entries have strict validation: hours must be positive, dates must be valid, approval_status transitions must follow rules. Schema enforcement prevents agents or integrations from creating invalid entries

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Projects | 50-500 active |
| Tasks | 500-10K |
| Time Entries | 10K-500K per year (grows linearly with headcount) |
| Billing Rates | 100-1K |
| Read:Write ratio | 4:1 (writes concentrated during work hours, reads for reporting and invoicing) |
| Query patterns | Filter by date range/user/project/approval status, aggregate hours by project/task/user, traverse project-task-entry chain |

---

## 5. ERP (Enterprise Resource Planning)

### Sample Entity Schemas

#### Product

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `sku` | `string` | Unique stock-keeping unit |
| `name` | `string` | Required |
| `description` | `string` | |
| `category` | `string` | `"raw-material"`, `"component"`, `"finished-good"`, `"service"` |
| `unit_of_measure` | `string` | `"each"`, `"kg"`, `"meter"`, `"liter"` |
| `cost` | `object` | `{ standard: decimal, average: decimal, last_purchase: decimal, currency: string }` |
| `pricing` | `object` | `{ list_price: decimal, min_price: decimal, currency: string }` |
| `weight` | `object\|null` | `{ value: decimal, unit: "kg"\|"lb" }` |
| `dimensions` | `object\|null` | `{ length, width, height, unit: "cm"\|"in" }` |
| `active` | `bool` | |

#### Inventory Record

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `warehouse_code` | `string` | `"WH-EAST"`, `"WH-WEST"` |
| `location_bin` | `string` | `"A-03-12"` (aisle-rack-bin) |
| `quantity_on_hand` | `decimal` | Current physical count |
| `quantity_reserved` | `decimal` | Allocated to orders not yet shipped |
| `quantity_available` | `decimal` | on_hand - reserved |
| `reorder_point` | `decimal` | Trigger replenishment below this |
| `last_counted_at` | `datetime` | Physical inventory date |

#### Order

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `order_number` | `string` | `"SO-2026-1234"` or `"PO-2026-0567"` |
| `type` | `string` | `"sales"` or `"purchase"` |
| `status` | `string` | `"draft"`, `"confirmed"`, `"in_production"`, `"shipped"`, `"delivered"`, `"cancelled"` |
| `line_items` | `array<object>` | `[{ product_sku: string, quantity: decimal, unit_price: decimal, discount_pct: decimal, line_total: decimal }]` |
| `shipping` | `object` | `{ method: string, tracking_number: string\|null, estimated_delivery: date, address: object }` |
| `total` | `decimal` | |
| `currency` | `string` | |
| `ordered_date` | `date` | |
| `required_date` | `date` | |

#### Bill of Materials (BOM)

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | `"Widget-A Assembly"` |
| `revision` | `string` | `"rev-3"` |
| `effective_date` | `date` | When this BOM version takes effect |
| `components` | `array<object>` | `[{ product_sku: string, quantity_per: decimal, unit: string, scrap_factor: decimal, notes: string }]` |
| `yield_quantity` | `decimal` | How many finished goods this BOM produces |
| `active` | `bool` | |

#### Supplier

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | Required |
| `code` | `string` | Short supplier code |
| `lead_time_days` | `int` | Standard lead time |
| `payment_terms` | `string` | `"net-30"`, `"net-60"` |
| `quality_rating` | `float\|null` | 0.0-5.0 |
| `address` | `object` | `{ street, city, state, postal_code, country }` |
| `contacts` | `array<object>` | `[{ name: string, email: string, phone: string, role: string }]` |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `stocked-at` | Product | Inventory Record | `{}` | Product-warehouse mapping |
| `ordered-from` | Order (purchase) | Supplier | `{ buyer: string }` | Purchase order to supplier |
| `ordered-by` | Order (sales) | Customer | `{}` | Sales order to customer |
| `produced-from` | Product | BOM | `{ revision: string }` | Finished good to its recipe |
| `component-of` | Product | BOM | `{ quantity_per: decimal }` | Raw material in a BOM |

### Key Workflows

**1. Sales Order Fulfillment with Inventory Reservation**

A sales order is confirmed and inventory is reserved:

1. `BEGIN TRANSACTION`
2. `UPDATE orders/so-1234 SET status = "confirmed" WHERE _version = 1`
3. For each line item:
   - Query `inventory-records` where product matches and `quantity_available >= order_quantity`
   - `UPDATE inventory-records/inv-xxx SET quantity_reserved = quantity_reserved + order_qty, quantity_available = quantity_available - order_qty WHERE _version = N`
4. `COMMIT`

OCC ensures that two concurrent orders cannot double-reserve the same inventory. If a version conflict occurs on the inventory record, the transaction rolls back and the agent retries with current stock levels.

**2. MRP (Material Requirements Planning) Explosion**

Determine raw materials needed for a production order:

1. Read the BOM for the target finished good via `produced-from` link
2. For each component in the BOM, follow `component-of` links recursively (sub-assemblies have their own BOMs)
3. At each level, multiply `quantity_per` by the production order quantity
4. Query inventory records via `stocked-at` links to find current availability
5. Net requirement = (required quantity * (1 + scrap_factor)) - quantity_available
6. Generate purchase orders for net requirements:
   - `BEGIN TRANSACTION`
   - `CREATE orders { type: "purchase", order_number: "PO-...", line_items: [...] }`
   - `CREATE LINK ordered-from FROM orders/po-new TO suppliers/supplier-xxx { buyer: "procurement-agent" }`
   - `COMMIT`

BOM explosion is recursive graph traversal — exactly the kind of multi-hop link query Axon is designed for.

**3. Inventory Adjustment with Audit**

Physical count reveals a discrepancy:

1. `BEGIN TRANSACTION`
2. `UPDATE inventory-records/inv-xxx SET quantity_on_hand = 95, quantity_available = 95 - quantity_reserved, last_counted_at = now() WHERE _version = N`
3. `COMMIT`

Audit log captures: who performed the count, the before/after quantities, and when. This is mandatory for inventory valuation and financial reporting. The diff shows exactly which fields changed.

### Why Axon Fits

- **Transactions**: Inventory reservation is the classic concurrency problem. Two sales orders for the last 10 units must not both succeed. OCC with version-based conflict detection prevents overselling without pessimistic locks that serialize the entire warehouse
- **Links**: BOM explosion is recursive graph traversal. Product-supplier-order relationships are naturally directional. "Which suppliers provide components for this finished good?" is a multi-hop link query, not a JOIN
- **Audit**: Inventory adjustments, cost changes, and order modifications all require audit trails for financial reporting and regulatory compliance (Sarbanes-Oxley for public companies). Axon's before/after audit is exactly what ERP auditors expect
- **Schema**: ERP has dozens of interrelated entity types with strict validation rules. A BOM component referencing a non-existent SKU, or an order line with negative quantity, must be rejected at the schema level

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Products | 1K-100K SKUs |
| Inventory Records | 5K-500K (products x warehouses x bins) |
| Orders | 10K-1M per year |
| BOMs | 500-50K |
| Suppliers | 100-10K |
| Read:Write ratio | 8:1 (heavy reads for availability checks, planning queries; batch writes during order processing) |
| Query patterns | Inventory availability by product/warehouse, BOM explosion (recursive link traversal), order status filtering, aggregate inventory valuation |

---

## 6. Issue Tracking

### Sample Entity Schemas

#### Issue

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `key` | `string` | Human-readable: `"AXON-142"` |
| `type` | `string` | `"bug"`, `"feature"`, `"task"`, `"epic"`, `"story"` |
| `title` | `string` | Required |
| `description` | `string` | Markdown body |
| `status` | `string` | `"open"`, `"in_progress"`, `"in_review"`, `"done"`, `"closed"`, `"wont_fix"` |
| `priority` | `string` | `"critical"`, `"high"`, `"medium"`, `"low"` |
| `labels` | `array<string>` | `["backend", "performance", "p0"]` |
| `story_points` | `int\|null` | Estimation |
| `assignee` | `string\|null` | User ID |
| `reporter` | `string` | User ID |
| `due_date` | `date\|null` | |
| `resolution` | `string\|null` | `"fixed"`, `"duplicate"`, `"wont_fix"`, `"cannot_reproduce"` |
| `environment` | `object\|null` | `{ os: string, browser: string, version: string }` |
| `attachments` | `array<object>` | `[{ filename: string, url: string, size_bytes: int, uploaded_at: datetime }]` |

#### Sprint

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | `"Sprint 14"` |
| `goal` | `string` | Sprint goal statement |
| `start_date` | `date` | |
| `end_date` | `date` | |
| `status` | `string` | `"planning"`, `"active"`, `"completed"` |
| `velocity` | `int\|null` | Story points completed (set at close) |

#### Comment

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `body` | `string` | Markdown |
| `author` | `string` | User ID |
| `edited` | `bool` | |

#### Project

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `key_prefix` | `string` | `"AXON"`, used in issue keys |
| `name` | `string` | |
| `description` | `string` | |
| `lead` | `string` | User ID |
| `default_assignee` | `string\|null` | |
| `status` | `string` | `"active"`, `"archived"` |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `belongs-to-project` | Issue | Project | `{}` | Issue-project membership |
| `in-sprint` | Issue | Sprint | `{ added_at: datetime }` | Sprint backlog membership |
| `blocks` | Issue | Issue | `{ reason: string\|null }` | Dependency / blocker |
| `is-child-of` | Issue | Issue | `{}` | Epic-story-subtask hierarchy |
| `commented-on` | Comment | Issue | `{}` | Comment-issue association |

### Key Workflows

**1. Sprint Planning**

Move issues into a sprint during planning:

1. `BEGIN TRANSACTION`
2. `UPDATE sprints/sprint-14 SET status = "active" WHERE _version = 1`
3. For each selected issue:
   - `CREATE LINK in-sprint FROM issues/AXON-142 TO sprints/sprint-14 { added_at: now() }`
   - `UPDATE issues/AXON-142 SET status = "open" WHERE _version = N` (if it was backlog)
4. `COMMIT`

Atomicity ensures the sprint and all its issue assignments are consistent. No partially-planned sprints.

**2. Issue State Transition with Blocker Check**

An engineer moves an issue to `in_progress`:

1. Query `blocks` links targeting this issue: `FOLLOW blocks TO issues/AXON-142`
2. Check if any blocking issue has status != `done` or `closed`
3. If blocked: reject transition with structured error listing blocking issues
4. If clear:
   - `BEGIN TRANSACTION`
   - `UPDATE issues/AXON-142 SET status = "in_progress", assignee = "eng-jane" WHERE _version = 5`
   - `COMMIT`

Audit log captures every state transition with actor and timestamp — essential for cycle time analysis and process compliance.

**3. Sprint Retrospective Metrics**

At sprint close, compute velocity and metrics:

1. Follow `in-sprint` links from all issues to `sprints/sprint-14`
2. Filter: issues where `status = "done"` or `status = "closed"`
3. Aggregate: `SUM(story_points)` for completed issues = velocity
4. `UPDATE sprints/sprint-14 SET status = "completed", velocity = 42 WHERE _version = 3`

Graph traversal: "Show me all issues in sprint 14 that are blocked by issues in sprint 13" requires multi-hop link queries across `in-sprint` and `blocks` link types.

### Why Axon Fits

- **Links**: Issue tracking is inherently a graph. `blocks`, `is-child-of`, `in-sprint` — these are typed directional relationships that Axon models natively. "What is blocking this epic's completion?" requires traversing `is-child-of` (epic -> stories) then `blocks` (blockers of each story) — a natural multi-hop query
- **Audit**: Every status change, reassignment, and priority shift is recorded with actor and timestamp. This enables cycle time analysis, SLA compliance tracking, and "who changed this and when" debugging. The audit log is the source of truth for engineering metrics
- **Schema**: Issue schemas evolve (new fields, new statuses) but must remain valid. Schema enforcement prevents integrations from creating issues with invalid status values or missing required fields
- **Transactions**: Sprint planning is a batch operation that must be atomic. Moving 20 issues into a sprint should not leave the sprint in a half-populated state if the operation fails midway

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Issues | 10K-500K (grows over project lifetime) |
| Sprints | 50-200 per year per project |
| Comments | 50K-2M |
| Projects | 5-100 |
| Read:Write ratio | 15:1 (boards, dashboards, search dominate; writes are individual status changes) |
| Query patterns | Filter by status/assignee/sprint/label, board views (issues grouped by status), blocker graph traversal, velocity aggregation by sprint |

---

## 7. Document Management

### Sample Entity Schemas

#### Document

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `title` | `string` | Required |
| `mime_type` | `string` | `"application/pdf"`, `"text/markdown"`, `"image/png"` |
| `status` | `string` | `"draft"`, `"in_review"`, `"approved"`, `"published"`, `"archived"`, `"superseded"` |
| `classification` | `string` | `"public"`, `"internal"`, `"confidential"`, `"restricted"` |
| `metadata` | `object` | `{ author: string, department: string, document_type: string, effective_date: date, expiry_date: date\|null }` |
| `tags` | `array<string>` | |
| `current_version_number` | `int` | Incremented on each new version |
| `storage_ref` | `string` | Reference to blob storage for actual file content |
| `size_bytes` | `int` | |
| `checksum` | `string` | SHA-256 of content |

#### Document Version

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `version_number` | `int` | 1, 2, 3... |
| `storage_ref` | `string` | Blob storage reference |
| `size_bytes` | `int` | |
| `checksum` | `string` | SHA-256 |
| `change_summary` | `string` | What changed in this version |
| `uploaded_by` | `string` | User ID |
| `uploaded_at` | `datetime` | |

#### Folder

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | Required |
| `path` | `string` | Materialized path: `"/legal/contracts/2026"` |
| `description` | `string\|null` | |
| `default_classification` | `string\|null` | Inherited by documents placed here |

#### Review

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `status` | `string` | `"pending"`, `"approved"`, `"rejected"`, `"changes_requested"` |
| `reviewer` | `string` | User ID |
| `decision_at` | `datetime\|null` | |
| `comments` | `string\|null` | Reviewer notes |
| `review_type` | `string` | `"content"`, `"legal"`, `"compliance"` |

#### Permission

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `principal` | `string` | User ID, group ID, or role |
| `principal_type` | `string` | `"user"`, `"group"`, `"role"` |
| `permission` | `string` | `"read"`, `"write"`, `"review"`, `"admin"` |
| `inherited` | `bool` | Inherited from parent folder |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `contained-in` | Document | Folder | `{}` | Document-folder placement |
| `has-version` | Document | Document Version | `{}` | Version chain |
| `reviewed-by` | Document | Review | `{}` | Review assignment |
| `references` | Document | Document | `{ context: string }` | Cross-reference between documents |
| `has-permission` | Document or Folder | Permission | `{}` | Access control binding |

### Key Workflows

**1. Document Review and Approval**

A document is submitted for multi-stage review:

1. Author uploads new version:
   - `BEGIN TRANSACTION`
   - `CREATE document-versions { version_number: 3, storage_ref: "s3://...", change_summary: "Updated section 4 per legal feedback" }`
   - `UPDATE documents/doc-789 SET status = "in_review", current_version_number = 3, storage_ref = "s3://...", checksum = "abc..." WHERE _version = 5`
   - `CREATE LINK has-version FROM documents/doc-789 TO document-versions/dv-new`
   - `COMMIT`
2. Create review assignments:
   - `CREATE reviews { status: "pending", reviewer: "legal-bob", review_type: "legal" }`
   - `CREATE LINK reviewed-by FROM documents/doc-789 TO reviews/rev-new`
3. Reviewer approves:
   - `BEGIN TRANSACTION`
   - `UPDATE reviews/rev-legal SET status = "approved", decision_at = now(), comments = "Looks good" WHERE _version = 1`
   - Check if all reviews for this document are approved
   - If all approved: `UPDATE documents/doc-789 SET status = "approved" WHERE _version = 6`
   - `COMMIT`

Audit trail shows who approved what, when, and which version — critical for regulated industries (pharma, finance, legal).

**2. Folder Permission Inheritance**

When a document is placed in a folder:

1. `BEGIN TRANSACTION`
2. `CREATE LINK contained-in FROM documents/doc-new TO folders/folder-legal-contracts`
3. Query permissions on the folder via `has-permission` links
4. For each inherited permission: `CREATE permissions { principal: "...", permission: "read", inherited: true }`
5. `CREATE LINK has-permission FROM documents/doc-new TO permissions/perm-new`
6. `COMMIT`

Atomically placing a document and establishing its permission set ensures no document exists without proper access controls.

**3. Document Lifecycle Query**

"Show me all confidential documents in /legal/ that are past their expiry date and still published":

1. Traverse `contained-in` links to find all documents in folder `/legal/` (and sub-folders via recursive folder containment)
2. Filter: `classification = "confidential" AND status = "published" AND metadata.expiry_date < now()`
3. Result: documents requiring action (archive or re-approve)

### Why Axon Fits

- **Links**: Document management is a graph of documents, folders, versions, reviews, and permissions. Version chains, cross-references, and permission inheritance are all typed directional links. "Show me the review history of this document" is a link traversal, not a JOIN
- **Audit**: Regulated industries (healthcare, finance, government) require knowing who accessed, modified, approved, or downloaded each document. Axon's immutable audit log with before/after state is precisely what compliance officers and auditors need
- **Transactions**: Multi-reviewer approval must be atomic — the document status flips to "approved" only when all reviews are approved, in a single transaction. Version upload + metadata update must be atomic to prevent orphaned versions
- **Schema**: Document classification, status, and permission values are strictly enumerated. A document with `classification: "topsecret"` (not a valid value) must be rejected. Schema enforcement prevents data quality issues that break access control

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Documents | 10K-1M |
| Document Versions | 50K-5M (2-5 versions per document on average) |
| Folders | 500-50K |
| Reviews | 20K-500K |
| Permissions | 50K-1M |
| Read:Write ratio | 20:1 (heavy reads for browsing, search; writes for uploads and reviews) |
| Query patterns | Filter by status/classification/folder, traverse version history, permission checks on access, review status aggregation |

---

## 8. MDM (Master Data Management)

### Sample Entity Schemas

#### Golden Record

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `domain` | `string` | `"customer"`, `"product"`, `"vendor"`, `"location"` |
| `canonical_data` | `object` | Domain-specific: `{ name, address, identifiers, classification, ... }` — varies by domain |
| `data_quality_score` | `float` | 0.0-1.0 |
| `completeness` | `object` | `{ total_fields: int, populated_fields: int, score: float }` |
| `survivorship_rules` | `object` | `{ field_precedence: { name: "crm", address: "erp", ... } }` — which source wins per field |
| `last_steward_review` | `datetime\|null` | When a data steward last reviewed this record |
| `status` | `string` | `"active"`, `"under_review"`, `"merged"`, `"deprecated"` |

#### Source Record

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `source_system` | `string` | `"salesforce"`, `"netsuite"`, `"shopify"`, `"manual_import"` |
| `source_id` | `string` | ID in the source system |
| `domain` | `string` | Same domain as parent golden record |
| `raw_data` | `object` | Original record from source |
| `normalized_data` | `object` | After standardization (address normalization, name parsing) |
| `ingested_at` | `datetime` | |
| `quality_flags` | `array<string>` | `["missing_zip", "invalid_phone", "possible_duplicate"]` |

#### Match Rule

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | `"exact-email-match"`, `"fuzzy-name-address"` |
| `domain` | `string` | Which entity domain this rule applies to |
| `match_fields` | `array<object>` | `[{ field: "email", algorithm: "exact" }, { field: "name", algorithm: "jaro-winkler", threshold: 0.85 }]` |
| `confidence_threshold` | `float` | Minimum score to auto-merge (e.g., 0.95) |
| `review_threshold` | `float` | Score above this needs steward review (e.g., 0.70) |
| `active` | `bool` | |
| `priority` | `int` | Rule evaluation order |

#### Merge Event

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `merge_type` | `string` | `"auto"`, `"manual"`, `"unmerge"` |
| `match_rule_used` | `string\|null` | ID of match rule (null for manual) |
| `confidence_score` | `float\|null` | Match confidence |
| `steward` | `string\|null` | User who approved (for manual merges) |
| `merged_at` | `datetime` | |
| `survivor_snapshot` | `object` | Golden record state after merge |
| `victim_snapshot` | `object` | Source record state before merge |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `sourced-from` | Golden Record | Source Record | `{ confidence: float, match_rule: string, linked_at: datetime }` | Which source records compose this golden record |
| `merged-into` | Golden Record (victim) | Golden Record (survivor) | `{ merge_event_id: string }` | Merge lineage |
| `matched-by` | Source Record | Source Record | `{ confidence: float, rule: string, status: "auto_merged"\|"pending_review"\|"rejected" }` | Match candidates |
| `governed-by` | Golden Record | Match Rule | `{}` | Which rules apply to this domain |

### Key Workflows

**1. Source Record Ingestion and Auto-Match**

A new customer record arrives from Salesforce:

1. `CREATE source-records { source_system: "salesforce", source_id: "003xx...", raw_data: {...}, normalized_data: {...} }`
2. Evaluate active match rules for domain `"customer"`:
   - Run `exact-email-match`: query golden records where `canonical_data.email = normalized_data.email`
   - Run `fuzzy-name-address`: query golden records with Jaro-Winkler similarity on name + address
3. If match confidence >= 0.95 (auto-merge threshold):
   - `BEGIN TRANSACTION`
   - `UPDATE golden-records/gr-123` — apply survivorship rules to merge fields from new source
   - `CREATE LINK sourced-from FROM golden-records/gr-123 TO source-records/sr-new { confidence: 0.97, match_rule: "exact-email-match" }`
   - `CREATE merge-events { merge_type: "auto", match_rule_used: "exact-email-match", confidence_score: 0.97, survivor_snapshot: {...}, victim_snapshot: {...} }`
   - `UPDATE source-records/sr-new SET quality_flags = [...] WHERE _version = 1`
   - `COMMIT`
4. If confidence between 0.70-0.95: create `matched-by` link with status `"pending_review"` for steward review
5. If no match: create new golden record and link

Audit trail captures every match decision for regulatory review and match rule tuning.

**2. Steward-Assisted Merge**

A data steward reviews a pending match:

1. Query `matched-by` links where `status = "pending_review"` — these are match candidates awaiting human review
2. Steward reviews source records side-by-side
3. If approved:
   - `BEGIN TRANSACTION`
   - `UPDATE matched-by link SET status = "auto_merged"`
   - `UPDATE golden-records/gr-survivor` — apply survivorship rules
   - `CREATE LINK sourced-from FROM golden-records/gr-survivor TO source-records/sr-xxx { confidence: 0.82, match_rule: "fuzzy-name-address" }`
   - `CREATE merge-events { merge_type: "manual", steward: "steward-jane", ... }`
   - `COMMIT`
4. If rejected: `UPDATE matched-by link SET status = "rejected"` — prevents re-surfacing

**3. Unmerge (Undo Incorrect Merge)**

A merge was incorrect — the two source records represent different people:

1. `BEGIN TRANSACTION`
2. Read the merge event to find the victim snapshot
3. `CREATE golden-records { ... }` — recreate the golden record from the victim snapshot
4. Move the relevant `sourced-from` links from the survivor to the new golden record
5. `CREATE merge-events { merge_type: "unmerge", ... }`
6. `CREATE LINK merged-into FROM new-golden-record TO survivor { merge_event_id: "..." }` — preserve lineage even after unmerge
7. `COMMIT`

### Why Axon Fits

- **Links with metadata**: MDM is fundamentally about links — which source records compose which golden records, at what confidence, via which match rule. These links are not just foreign keys; they carry `confidence`, `match_rule`, and `linked_at` metadata that is queried, reported, and audited. Axon's first-class link model is a natural fit
- **Audit**: Every merge decision, survivorship rule application, and data quality change must be traceable. Regulators and data stewards need to answer "why does this golden record look like this?" The answer is in the audit log and merge event history
- **Transactions**: Merge and unmerge operations involve multiple entities and links that must be atomically consistent. A half-completed merge produces corrupt master data that propagates to every downstream system
- **Schema**: Golden records have domain-specific schemas that must be strictly enforced. A customer golden record missing a `canonical_email` or with an invalid `data_quality_score` breaks downstream matching and activation. Schema validation is the first line of defense

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Golden Records | 100K-10M per domain |
| Source Records | 500K-50M (multiple sources per golden record) |
| Match Rules | 10-100 |
| Merge Events | 50K-5M |
| Read:Write ratio | 2:1 (heavy writes during ingestion and matching; heavy reads during steward review and downstream syndication) |
| Query patterns | Identity lookup by various identifiers, match candidate retrieval for steward queue, lineage traversal (golden record -> source records), data quality aggregation by source system |

---

## 9. Workflow Automation

### Sample Entity Schemas

#### Workflow Definition

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `name` | `string` | `"invoice-approval"`, `"employee-onboarding"`, `"document-review"` |
| `description` | `string` | |
| `version_tag` | `string` | `"v2.1"` — definition versioning |
| `status` | `string` | `"draft"`, `"active"`, `"deprecated"` |
| `trigger` | `object` | `{ type: "manual"\|"event"\|"schedule", config: { event_type: string, cron: string, ... } }` |
| `steps` | `array<object>` | (see below) |
| `variables` | `array<object>` | `[{ name: string, type: string, default: any, required: bool }]` |

Step structure (nested within `steps`):

| Field | Type | Notes |
|-------|------|-------|
| `step_id` | `string` | Unique within this definition |
| `name` | `string` | `"manager-approval"`, `"send-notification"` |
| `type` | `string` | `"human_task"`, `"automated_action"`, `"condition"`, `"parallel_gateway"`, `"wait"` |
| `config` | `object` | Type-specific: `{ assignee_rule, action_type, condition_expr, timeout, ... }` |
| `on_complete` | `string\|null` | Next step_id |
| `on_timeout` | `string\|null` | Step_id if step times out |
| `on_error` | `string\|null` | Step_id on error |

#### Workflow Instance

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `definition_id` | `string` | Which workflow definition (and version) |
| `status` | `string` | `"running"`, `"paused"`, `"completed"`, `"failed"`, `"cancelled"` |
| `current_step_id` | `string` | Active step |
| `variables` | `object` | Runtime variable state: `{ invoice_id: "...", approved: true, amount: 5000.00 }` |
| `started_at` | `datetime` | |
| `completed_at` | `datetime\|null` | |
| `started_by` | `string` | Actor who triggered this instance |
| `error` | `object\|null` | `{ step_id: string, message: string, occurred_at: datetime }` |

#### Step Execution

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `step_id` | `string` | Refers to step within workflow definition |
| `status` | `string` | `"pending"`, `"in_progress"`, `"completed"`, `"failed"`, `"skipped"`, `"timed_out"` |
| `assigned_to` | `string\|null` | For human tasks |
| `started_at` | `datetime\|null` | |
| `completed_at` | `datetime\|null` | |
| `input` | `object` | Variables snapshot at step start |
| `output` | `object\|null` | Result produced by this step |
| `decision` | `string\|null` | For human tasks: `"approve"`, `"reject"`, `"escalate"` |

#### Action

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `type` | `string` | `"send_email"`, `"create_entity"`, `"update_entity"`, `"call_webhook"`, `"assign_task"` |
| `config` | `object` | `{ template_id, entity_collection, webhook_url, ... }` |
| `status` | `string` | `"pending"`, `"executed"`, `"failed"` |
| `result` | `object\|null` | Action outcome |
| `executed_at` | `datetime\|null` | |
| `retries` | `int` | Number of retry attempts |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `instance-of` | Workflow Instance | Workflow Definition | `{ definition_version: string }` | Which definition this instance runs |
| `executed-step` | Workflow Instance | Step Execution | `{ sequence: int }` | Ordered step execution history |
| `triggered-action` | Step Execution | Action | `{}` | Actions performed by a step |
| `operates-on` | Workflow Instance | (any entity) | `{ role: string }` | The business entity this workflow processes |
| `escalated-to` | Step Execution | Step Execution | `{ reason: string }` | Escalation path |

### Key Workflows

**1. Invoice Approval Workflow Execution**

A workflow instance progresses through approval steps:

1. Trigger: new invoice created -> workflow engine creates instance:
   - `BEGIN TRANSACTION`
   - `CREATE workflow-instances { definition_id: "invoice-approval", status: "running", current_step_id: "manager-approval", variables: { invoice_id: "inv-042", amount: 5000.00 }, started_by: "system" }`
   - `CREATE LINK instance-of FROM workflow-instances/wi-new TO workflow-definitions/wd-invoice-approval { definition_version: "v2.1" }`
   - `CREATE LINK operates-on FROM workflow-instances/wi-new TO invoices/inv-042 { role: "subject" }`
   - `CREATE step-executions { step_id: "manager-approval", status: "pending", assigned_to: "manager-bob", input: { invoice_id: "inv-042", amount: 5000.00 } }`
   - `CREATE LINK executed-step FROM workflow-instances/wi-new TO step-executions/se-new { sequence: 1 }`
   - `COMMIT`

2. Manager approves:
   - `BEGIN TRANSACTION`
   - `UPDATE step-executions/se-approval SET status = "completed", decision = "approve", completed_at = now(), output = { approved: true } WHERE _version = 1`
   - Evaluate workflow definition: next step is "finance-review" (because amount > 1000)
   - `UPDATE workflow-instances/wi-123 SET current_step_id = "finance-review", variables.approved_by_manager = true WHERE _version = 2`
   - `CREATE step-executions { step_id: "finance-review", status: "pending", assigned_to: "finance-carol" }`
   - `CREATE LINK executed-step FROM workflow-instances/wi-123 TO step-executions/se-new-2 { sequence: 2 }`
   - `COMMIT`

**2. Workflow Definition Versioning**

Updating a workflow definition without breaking running instances:

1. `CREATE workflow-definitions { name: "invoice-approval", version_tag: "v2.2", status: "active", steps: [...updated...] }`
2. `UPDATE workflow-definitions/wd-v2.1 SET status = "deprecated" WHERE _version = N`
3. Running instances continue on v2.1 (their `instance-of` link points to the old definition). New instances use v2.2

Axon's entity versioning ensures the old definition is preserved. Audit log shows when and why the definition changed.

**3. Timeout and Escalation**

A step times out and escalates:

1. Scheduler detects `step-executions` where `status = "pending"` and `started_at + timeout < now()`
2. `BEGIN TRANSACTION`
3. `UPDATE step-executions/se-stale SET status = "timed_out" WHERE _version = N`
4. `CREATE step-executions { step_id: "director-escalation", status: "pending", assigned_to: "director-dave" }`
5. `CREATE LINK escalated-to FROM step-executions/se-stale TO step-executions/se-escalation { reason: "timeout after 48h" }`
6. `CREATE LINK executed-step FROM workflow-instances/wi-123 TO step-executions/se-escalation { sequence: 3 }`
7. `UPDATE workflow-instances/wi-123 SET current_step_id = "director-escalation" WHERE _version = M`
8. `COMMIT`

### Why Axon Fits

- **Links**: Workflow execution is a graph — instances link to definitions, step executions chain sequentially with escalation branches, actions link back to the business entities they modify. The `operates-on` link connects the abstract workflow to the concrete business entity, enabling queries like "show me all workflows that have operated on this invoice"
- **Audit**: Workflow compliance requires proving that every step was executed by the right person at the right time. The audit log on step execution entities provides this without additional logging infrastructure. For regulated workflows (SOX, HIPAA), this is a hard requirement
- **Transactions**: Step transitions must be atomic — updating the step execution, advancing the workflow instance, and creating the next step execution must all succeed or all fail. Partial workflow state produces stuck or inconsistent instances
- **Schema**: Workflow definitions are complex nested structures (steps with conditions, timeouts, error handling). Schema validation ensures that only valid workflow definitions are stored. Runtime variable schemas prevent steps from receiving unexpected input shapes

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Workflow Definitions | 20-200 |
| Workflow Instances | 10K-1M per year |
| Step Executions | 50K-5M per year (3-10 steps per instance) |
| Actions | 50K-5M per year |
| Read:Write ratio | 3:1 (writes during execution; reads for dashboards, queue views, audit) |
| Query patterns | Active instances by status/definition, pending step executions by assignee (task queue), instance history traversal, SLA breach detection by time |

---

## 10. Agentic Applications

### Sample Entity Schemas

#### Bead (Work Item)

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `type` | `string` | `"code-review"`, `"research"`, `"implementation"`, `"test"`, `"document"` |
| `status` | `string` | `"draft"`, `"pending"`, `"ready"`, `"in_progress"`, `"review"`, `"done"`, `"blocked"`, `"cancelled"` |
| `title` | `string` | Required |
| `description` | `string` | Markdown specification of what needs to be done |
| `content` | `object` | Type-specific payload: `{ code: string, language: string, file_path: string }` or `{ findings: array, sources: array }` |
| `priority` | `int` | 1 (highest) to 5 (lowest) |
| `assignee` | `string\|null` | Agent ID or user ID |
| `tags` | `array<string>` | `["backend", "axon", "p0"]` |
| `acceptance_criteria` | `array<string>` | `["All tests pass", "Coverage > 80%", "No new warnings"]` |
| `estimated_effort` | `string\|null` | `"small"`, `"medium"`, `"large"` |
| `metadata` | `object` | `{ source_session: string, parent_goal: string, retry_count: int }` |

#### Agent Session

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `agent_id` | `string` | Which agent: `"claude-code"`, `"research-agent"`, `"qa-agent"` |
| `model` | `string` | `"claude-opus-4-6"`, `"claude-sonnet-4"` |
| `status` | `string` | `"active"`, `"completed"`, `"failed"`, `"timed_out"` |
| `started_at` | `datetime` | |
| `ended_at` | `datetime\|null` | |
| `token_usage` | `object` | `{ input_tokens: int, output_tokens: int, cache_read_tokens: int, total_cost_usd: decimal }` |
| `goal` | `string` | Human-readable objective |
| `outcome` | `string\|null` | `"success"`, `"partial"`, `"failure"` |
| `error` | `object\|null` | `{ type: string, message: string, stack: string }` |
| `config` | `object` | `{ temperature: float, max_tokens: int, tools_enabled: array<string> }` |

#### Tool Call

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `tool_name` | `string` | `"bash"`, `"read"`, `"edit"`, `"grep"`, `"web_search"` |
| `input` | `object` | Tool-specific input parameters |
| `output` | `object\|null` | Tool result (may be truncated for large outputs) |
| `status` | `string` | `"pending"`, `"executing"`, `"completed"`, `"failed"`, `"timed_out"` |
| `started_at` | `datetime` | |
| `completed_at` | `datetime\|null` | |
| `duration_ms` | `int\|null` | |
| `error` | `object\|null` | `{ type: string, message: string }` |
| `sequence` | `int` | Order within session |

#### Chain-of-Thought Trace

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `reasoning` | `string` | The agent's reasoning text |
| `decision` | `string` | What the agent decided to do |
| `confidence` | `float\|null` | 0.0-1.0 self-assessed confidence |
| `alternatives_considered` | `array<object>` | `[{ option: string, reason_rejected: string }]` |
| `sequence` | `int` | Order within session |
| `timestamp` | `datetime` | |

#### Dependency DAG Node

| Field | Type | Notes |
|-------|------|-------|
| `_id` | `string (UUIDv7)` | System-managed |
| `_version` | `int` | OCC version |
| `dag_id` | `string` | Groups nodes into a DAG |
| `node_type` | `string` | `"bead"`, `"gate"`, `"checkpoint"` |
| `status` | `string` | `"waiting"`, `"ready"`, `"running"`, `"done"`, `"failed"` |
| `payload_ref` | `string\|null` | Entity ID of the associated bead/gate |

### Sample Link Types

| Link Type | Source | Target | Metadata | Notes |
|-----------|--------|--------|----------|-------|
| `depends-on` | Bead | Bead | `{ dependency_type: "hard"\|"soft", reason: string }` | DAG dependency |
| `worked-in` | Bead | Agent Session | `{ role: "primary"\|"reviewer" }` | Which session processed this bead |
| `called-tool` | Agent Session | Tool Call | `{ sequence: int }` | Session-to-tool-call chain |
| `reasoned-at` | Agent Session | Chain-of-Thought Trace | `{ sequence: int }` | Session-to-reasoning chain |
| `dag-edge` | Dependency DAG Node | Dependency DAG Node | `{ edge_type: "requires"\|"triggers" }` | DAG structure |

### Key Workflows

**1. Bead Decomposition and DAG Construction**

An orchestrator agent breaks a goal into beads:

1. `BEGIN TRANSACTION`
2. `CREATE beads { type: "implementation", title: "Implement entity CRUD", status: "pending", ... }`
3. `CREATE beads { type: "test", title: "Write entity CRUD tests", status: "pending", ... }`
4. `CREATE beads { type: "code-review", title: "Review entity CRUD", status: "pending", ... }`
5. `CREATE LINK depends-on FROM beads/bead-test TO beads/bead-impl { dependency_type: "hard", reason: "tests require implementation to exist" }`
6. `CREATE LINK depends-on FROM beads/bead-review TO beads/bead-test { dependency_type: "hard", reason: "review after tests pass" }`
7. `COMMIT`

Atomicity ensures the dependency graph is always consistent. No bead exists without its dependency links.

**2. Ready Queue Computation and Agent Dispatch**

Find beads ready for execution:

1. Query beads where `status = "pending"`
2. For each, traverse `depends-on` links to check if all dependencies have `status = "done"`
3. Beads with all dependencies satisfied are "ready" — update status:
   - `BEGIN TRANSACTION`
   - `UPDATE beads/bead-test SET status = "ready" WHERE _version = 3`
   - `CREATE agent-sessions { agent_id: "claude-code", model: "claude-opus-4-6", status: "active", goal: "Write entity CRUD tests" }`
   - `CREATE LINK worked-in FROM beads/bead-test TO agent-sessions/session-new { role: "primary" }`
   - `COMMIT`
4. OCC prevents two orchestrators from dispatching the same bead to different agents — the second `UPDATE` will fail with a version conflict

**3. Session Replay and Debugging**

When an agent produces a bad result, replay its reasoning:

1. Start from `beads/bead-xxx` — follow `worked-in` links to find agent sessions
2. For each session, follow `called-tool` links (ordered by `sequence`) to reconstruct the tool call sequence
3. For each session, follow `reasoned-at` links (ordered by `sequence`) to reconstruct the chain of thought
4. Combine into a chronological trace: reasoning -> tool call -> reasoning -> tool call -> ...

This is graph traversal over link types — exactly the query pattern Axon optimizes for. The audit log adds another dimension: "what did the agent *change* about the bead at each step?" via before/after diffs.

**4. Agent Cost and Performance Analysis**

Aggregate agent behavior across sessions:

1. Query agent sessions: `SELECT agent_id, model, SUM(token_usage.total_cost_usd), AVG(duration), COUNT(*) FROM agent-sessions WHERE started_at > now() - 7 days GROUP BY agent_id, model`
2. For sessions with `outcome = "failure"`, traverse `called-tool` links to find which tool calls failed
3. Cross-reference with beads via `worked-in` links: "Which bead types have the highest failure rate?"

**5. Dependency DAG Cycle Detection**

Before committing a new `depends-on` link, verify no cycle is introduced:

1. Proposed: `bead-C depends-on bead-A`
2. Traverse `depends-on` links forward from `bead-A` with depth limit
3. If `bead-C` is reachable from `bead-A`, the link would create a cycle — reject
4. If no cycle detected:
   - `BEGIN TRANSACTION`
   - `CREATE LINK depends-on FROM beads/bead-C TO beads/bead-A { dependency_type: "hard", reason: "..." }`
   - `COMMIT`

### Why Axon Fits

- **Links**: Agentic applications are graph-native. Bead dependency DAGs, session-to-tool-call chains, and chain-of-thought traces are all directed graphs. Axon's typed links with metadata model these naturally. The ready queue computation — "find beads with all dependencies done" — is a graph query that Axon handles as a first-class operation, not a workaround
- **Audit**: Agent observability demands knowing exactly what changed, when, and by which agent. The audit log provides a complete, immutable record of every bead mutation. When an agent produces a wrong result, the audit trail shows every intermediate state. This is the difference between "the output is wrong" and "I can see exactly where the agent went off track"
- **Transactions**: Bead decomposition (creating multiple beads with dependency links) must be atomic. Ready queue dispatch (claiming a bead for an agent) must be race-free. OCC prevents double-dispatch without pessimistic locking
- **Schema**: Agent output is notoriously variable. Schema enforcement on beads ensures that no matter what an agent produces, the stored state conforms to the expected structure. Tool call inputs and outputs have schemas. Chain-of-thought traces have schemas. Without this, downstream consumers break on unexpected shapes

### Scale Characteristics

| Metric | Typical Range |
|--------|---------------|
| Beads | 1K-100K per project |
| Agent Sessions | 10K-1M per month |
| Tool Calls | 100K-10M per month (5-50 per session) |
| Chain-of-Thought Traces | 50K-5M per month |
| DAG Nodes | 1K-50K |
| Read:Write ratio | 2:1 (heavy writes during agent execution; heavy reads for ready queue, replay, dashboards) |
| Query patterns | Ready queue (dependency graph traversal + status filter), session replay (ordered link traversal), bead status boards, cost aggregation by agent/model, cycle detection (reachability query) |

---

## Cross-Domain Summary

### Feature Importance by Domain

| Domain | Typed Links | ACID Transactions | Audit Log | Schema Validation |
|--------|:-----------:|:-----------------:|:---------:|:-----------------:|
| CRM | Critical | High | High | High |
| CDP | Critical | Critical | Critical | High |
| AP/AR | High | Critical | Critical | Critical |
| Time Tracking | High | High | Critical | High |
| ERP | Critical | Critical | Critical | Critical |
| Issue Tracking | Critical | Medium | High | High |
| Document Mgmt | Critical | High | Critical | High |
| MDM | Critical | Critical | Critical | Critical |
| Workflow Automation | Critical | Critical | Critical | High |
| Agentic Applications | Critical | High | Critical | Critical |

### Observations

1. **Links are universally critical.** Every domain models the world as entities and relationships. Typed, directional links with metadata are not a nice-to-have — they are the core data modeling primitive that distinguishes Axon from document stores and relational databases.

2. **Audit is non-negotiable for 8 of 10 domains.** Financial systems (AP/AR, ERP), regulated workflows (document management, workflow automation), and agent observability (agentic applications, CDP) all require immutable, queryable mutation history. This validates Axon's audit-first architecture.

3. **Transactions matter most for multi-entity consistency.** Payment application, inventory reservation, profile merges, workflow step transitions — these are the operations where partial failure produces corrupt state. Cross-collection transactions (debit accounts + create ledger entry) appear in 6 of 10 domains.

4. **Schema enforcement prevents downstream breakage.** Every domain benefits, but financial (AP/AR, ERP), master data (MDM, CDP), and agentic applications show the highest impact — these are domains where garbage data propagates to many consumers.

5. **Graph traversal patterns are consistent.** Across domains, the query patterns repeat: "follow typed links with depth limits and entity-level filters at each hop." BOM explosion, dependency DAGs, approval chains, identity resolution lineage, and document version history are all instances of the same traversal primitive.

6. **Scale is moderate.** The sweet spot across all domains is thousands to low millions of entities per collection — exactly Axon's design target. None of these domains require warehouse-scale analytics; they need transactional correctness at moderate scale.

---

*This document informs feature specifications in `docs/helix/01-frame/features/` and user stories derived from them.*
