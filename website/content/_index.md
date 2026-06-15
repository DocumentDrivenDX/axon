---
title: Axon
layout: hextra-home
---

<section class="axon-hero" aria-labelledby="axon-hero-title">
  <div class="axon-hero__copy">
    <a class="axon-release-pill" href="docs/coverage/" aria-label="Open HELIX 0.7.1 coverage">
      <span>HELIX 0.7.1 coverage</span>
      <strong>198 mapped proofs</strong>
    </a>
    <h1 id="axon-hero-title">Governed state for agents that write business records.</h1>
    <p>
      Axon gives developers one request path for schema validation, policy
      decisions, mutation previews, approval routing, version checks, and
      repair-grade audit evidence.
    </p>
    <div class="axon-actions" aria-label="Primary pages">
      <a class="axon-button axon-button--primary" href="docs/getting-started/">Run the quickstart</a>
      <a class="axon-button axon-button--secondary" href="docs/examples/">Review sample projects</a>
      <a class="axon-button axon-button--secondary" href="docs/demo-reels/">Watch demo reels</a>
    </div>
  </div>

  <div class="axon-hero__surface" aria-label="Axon governed write request path">
    <div class="axon-console">
      <div class="axon-console__bar">
        <span></span><span></span><span></span>
        <strong>agent-write.axon</strong>
      </div>
      <div class="axon-console__body">
        <p><span class="axon-prompt">$</span> axon intents preview invoice-1042 --actor ap-agent</p>
        <p><span class="axon-ok">schema</span> Invoice.v7 accepted 6 fields and 2 links</p>
        <p><span class="axon-warn">policy</span> amount_change requires finance approval</p>
        <p><span class="axon-ok">version</span> entity:91 policy:18 grant:44 operation:stable</p>
      </div>
    </div>
    <ol class="axon-request-path" aria-label="Governed write stages">
      <li><span>01</span><strong>Schema</strong><em>typed entity and links</em></li>
      <li><span>02</span><strong>Policy</strong><em>field visibility and action decision</em></li>
      <li><span>03</span><strong>Intent</strong><em>diff, pre-image, version token</em></li>
      <li><span>04</span><strong>Approval</strong><em>human review or direct commit</em></li>
      <li><span>05</span><strong>Audit</strong><em>repairable before and after record</em></li>
    </ol>
  </div>
</section>

<section class="axon-proof-strip" aria-label="HELIX coverage proof points">
  <a href="docs/coverage/"><strong>31</strong><span>feature specs</span></a>
  <a href="docs/coverage/"><strong>140</strong><span>user stories</span></a>
  <a href="docs/coverage/"><strong>17</strong><span>scenarios</span></a>
  <a href="docs/examples/"><strong>100%</strong><span>mapped examples</span></a>
</section>

<section class="axon-section axon-section--split" aria-labelledby="write-path-title">
  <div>
    <p class="axon-eyebrow">shared human and agent workflow</p>
    <h2 id="write-path-title">One governed write path across MCP, GraphQL, CLI, and apps.</h2>
    <p>
      Agents can discover tools and propose writes without bypassing the
      controls used by operators and application code. Axon rechecks schema,
      policy, grants, operation shape, and entity versions before a mutation
      commits.
    </p>
  </div>
  <div class="axon-check-grid">
    <div><strong>Preview first</strong><span>Every risky write can become an inspectable intent.</span></div>
    <div><strong>Review with context</strong><span>Approvals bind to the reviewed pre-image and policy version.</span></div>
    <div><strong>Commit safely</strong><span>Stale entity, schema, grant, and operation versions are rejected.</span></div>
    <div><strong>Repair later</strong><span>Audit records keep actor, tool, policy, and before/after evidence.</span></div>
  </div>
</section>

<section class="axon-section" aria-labelledby="coverage-title">
  <div class="axon-section__header">
    <p class="axon-eyebrow">documentation as evidence</p>
    <h2 id="coverage-title">The microsite maps product claims to inspectable evidence.</h2>
    <p>
      Generated HELIX pages connect requirements to CLI flows, sample projects,
      and demo reels for schema design, policy guardrails, audit evidence,
      tenant control, local operation, and agent taskboards.
    </p>
  </div>
  <div class="axon-card-grid">
    <a class="axon-card" href="docs/coverage/">
      <span>Coverage</span>
      <strong>Feature, story, scenario, and use-case traceability</strong>
    </a>
    <a class="axon-card" href="docs/examples/">
      <span>Examples</span>
      <strong>Worked projects with setup commands and expected audit output</strong>
    </a>
    <a class="axon-card" href="docs/demo-reels/">
      <span>Demo reels</span>
      <strong>Scenario walkthroughs for governed write and review paths</strong>
    </a>
  </div>
</section>

<section class="axon-section axon-section--install" aria-labelledby="install-title">
  <div>
    <p class="axon-eyebrow">local governed state</p>
    <h2 id="install-title">Start with a local entity store, then keep the same controls.</h2>
  </div>

```bash
curl -sf https://DocumentDrivenDX.github.io/axon/install.sh | sh
axon serve --no-auth --storage memory
axon collections create tasks
axon schema set tasks --schema '{"type":"object","properties":{"title":{"type":"string"},"status":{"type":"string"}},"required":["title","status"]}'
axon entities create tasks --id task-001 --data '{"title":"Ship it","status":"open"}'
```
</section>
