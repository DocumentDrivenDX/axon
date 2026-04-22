---
ddx:
  id: helix.schema-format-research
  depends_on:
    - helix.product-vision
---
# Research: Schema Format Landscape for Entity-Graph-Relational Data Models

**Version**: 0.2.0
**Date**: 2026-04-04
**Status**: Draft
**Author**: Erik LaBianca

---

## 1. Purpose

Axon needs a schema format -- Entity Schema Format (ESF) -- that defines entity structures, link types, validation rules, and cardinality constraints for an entity-graph-relational data model. This document surveys existing schema formats and graph systems to identify patterns worth adopting, traps worth avoiding, and the specific design decisions ESF must make.

Axon's schema system must support:
- **Deeply nested entity structures** (up to 8 levels, recursive types)
- **Typed directional links** between entities with link-type schemas, cardinality, and metadata
- **Validation rules** with severity levels (error/warning/info), cross-field constraints, context-specific constraints (e.g., per-LOB nullability)
- **Schema evolution** (additive safe, breaking detected)
- **Multi-format bridging** (generate SQL DDL, JSON Schema, Protobuf, TypeScript from one definition)
- **Agent-friendly** -- schemas must be introspectable by AI agents, errors must be structured and actionable
- **Embeddable** -- schema validation must run in-process in Rust with sub-millisecond performance

The systems surveyed fall into four categories:

1. **Graph/semantic schemas** -- OWL, RDFS, SHACL, ShEx, JSON-LD, PG-Schema
2. **Document/object schemas** -- JSON Schema, TypeBox, Zod/Valibot, Protobuf, Avro, Cap'n Proto/FlatBuffers
3. **Database-native schemas** -- EdgeDB SDL, TypeDB TypeQL, SurrealDB DEFINE, Prisma Schema
4. **Data pipeline schemas** -- UMF (tablespec), Great Expectations, dbt schema.yml

Additionally, three graph systems are examined in depth for their schema design lessons: Stardog, TerminusDB, and Cayley.

---

## 2. Evaluation Criteria

Each format is scored on a 5-point scale (1 = poor fit, 5 = excellent fit) across:

| Criterion | What It Measures |
|-----------|-----------------|
| **Entity-Graph Fitness** | Natural expression of nested entities + typed links + link metadata |
| **Validation Power** | Field-level, cross-field, cross-entity, conditional/contextual constraints |
| **Evolution Model** | Schema versioning, additive safety, breaking change detection |
| **Rust Ecosystem** | Parser/validator crates, maturity, performance |
| **Agent Ergonomics** | Readability/writability by AI agents, structured error output |
| **Adoption/Community** | Ecosystem size, tooling, documentation quality |
| **Bridging Capability** | Generation of other formats (SQL, Protobuf, TypeScript, JSON Schema) |

---

## 3. Graph/Semantic Schema Formats

### 3.1 OWL (Web Ontology Language)

**What it is**: W3C standard for defining ontologies with description logic (DL) expressiveness. OWL 2 has three profiles (EL, QL, RL) with varying computational complexity. Defines class hierarchies, property restrictions (domain, range, cardinality), and axioms for reasoning.

**Entity-Graph Fitness (3/5)**:
OWL natively models classes (entities) and properties (relationships). Object properties are typed links between individuals; datatype properties are entity fields. Property restrictions can express cardinality (owl:minCardinality, owl:maxCardinality) and value constraints (owl:allValuesFrom, owl:someValuesFrom). However, OWL operates under the **Open World Assumption (OWA)** -- absence of a statement does not mean it is false. This is fundamentally at odds with Axon's closed-world validation requirement (a missing required field IS an error). Deeply nested structures are awkward: OWL models everything as flat triples, so nesting requires reification or blank nodes.

**Validation Power (2/5)**:
OWL is designed for inference, not validation. Cardinality restrictions are assertions about what COULD exist, not constraints on what MUST exist. Cross-field constraints require complex class expressions. Conditional/contextual constraints (per-LOB nullability) have no natural expression. OWL 2 RL can be used with closed-world reasoning, but this is non-standard usage.

**Evolution Model (2/5)**:
No built-in schema evolution mechanism. Ontology versioning is ad-hoc (owl:versionIRI). No breaking-change detection. Import chains (owl:imports) create fragile dependency graphs.

**Rust Ecosystem (1/5)**:
The `horned-owl` crate provides OWL 2 parsing (OWL/XML, Functional Syntax) and is the most mature option, but it focuses on ontology manipulation, not validation. No production-quality OWL reasoner exists in Rust. `sophia_rs` provides RDF graph support. Performance of DL reasoning is orders of magnitude too slow for sub-millisecond validation.

**Agent Ergonomics (1/5)**:
OWL serializations (RDF/XML, Turtle, Functional Syntax) are verbose and hostile to both human and AI readers. Manchester Syntax is more readable but poorly tooled. Error messages from OWL reasoners are logic-oriented ("unsatisfiable class"), not field-level.

**Adoption/Community (3/5)**:
Strong in academia, biomedical informatics, and knowledge graphs. Weak in application development. Large specification surface, multiple serializations, steep learning curve.

**Bridging Capability (2/5)**:
OWL-to-SQL mappings exist (e.g., OWL2SQL) but are research-quality. No production bridges to Protobuf or TypeScript. JSON-LD provides a JSON serialization path.

**Verdict**: OWL's open-world assumption, reasoning complexity, and poor Rust support make it unsuitable as Axon's schema format. The concepts of class hierarchies and property restrictions are informative for ESF design, but OWL itself is not adoptable.

---

### 3.2 RDFS (RDF Schema)

**What it is**: Lightweight vocabulary for describing RDF resources. Defines rdfs:Class, rdfs:subClassOf, rdfs:domain, rdfs:range, rdfs:label, rdfs:comment. Much simpler than OWL -- essentially class/property definitions without the DL expressiveness.

**Entity-Graph Fitness (2/5)**:
RDFS can express classes (entity types) and properties (fields/links) with domain and range constraints. But it has no cardinality constraints, no value restrictions, no closed-world validation, and no way to express nested structures without reification. Links between entities map naturally to RDF properties, but link metadata requires reification (turning a triple into an entity), which is cumbersome.

**Validation Power (1/5)**:
RDFS provides only domain/range type checking. No required fields, no enums, no patterns, no cross-field constraints. RDFS was designed for inference, not validation.

**Evolution Model (1/5)**:
No evolution support. Vocabulary changes are unmanaged.

**Rust Ecosystem (2/5)**:
`sophia_rs` provides RDF graph and RDFS inference support. `oxrdf`/`oxigraph` provide RDF storage and SPARQL. But RDFS validation is not a meaningful operation -- there is nothing substantive to validate.

**Agent Ergonomics (2/5)**:
Turtle syntax is reasonably readable for simple class hierarchies. But the lack of validation semantics means there is nothing to report.

**Adoption/Community (3/5)**:
Widely used as a foundation for other vocabularies (FOAF, Dublin Core, Schema.org). But always as a building block, never as a standalone schema system.

**Bridging Capability (1/5)**:
Too limited to generate meaningful SQL DDL, Protobuf, or TypeScript definitions.

**Verdict**: RDFS is too limited for Axon's needs. It provides useful concepts (class hierarchies, property domains/ranges) but lacks the validation and structural expressiveness required.

---

### 3.3 SHACL (Shapes Constraint Language)

**What it is**: W3C recommendation (2017) for validating RDF graphs against a set of conditions called "shapes." Operates under the **Closed World Assumption (CWA)**, making it validation-oriented rather than inference-oriented. Shapes define constraints on node properties: cardinality, value types, patterns, value ranges, and logical combinations.

**Entity-Graph Fitness (3/5)**:
SHACL can validate nested structures through sh:node (linking to other shapes). Typed links map to property shapes with sh:class constraints. Link metadata is awkward -- requires reified statements or intermediate nodes. Recursive shapes are supported (a shape can reference itself). However, SHACL validates existing RDF graphs rather than defining schemas generatively -- it is a constraint language, not a definition language.

**Validation Power (4/5)**:
SHACL's validation is strong. Field-level: sh:datatype, sh:minCount, sh:maxCount, sh:pattern, sh:in (enum), sh:minInclusive/sh:maxInclusive. Cross-field: SPARQL-based constraints (sh:sparql) enable arbitrary cross-field validation. Severity levels: sh:Violation, sh:Warning, sh:Info -- directly matching Axon's error/warning/info model. This is the closest match to Axon's validation requirements among all graph-oriented formats.

**Evolution Model (2/5)**:
No built-in evolution support. Shape changes are unversioned. Breaking-change detection would need to be built externally by comparing shape graphs.

**Rust Ecosystem (2/5)**:
No production SHACL validator in Rust. `rudof` (formerly `shex_rs`) includes partial SHACL support via its `shacl_validation` crate, but it is experimental. `oxigraph` provides RDF storage but no SHACL validation. A full SHACL implementation in Rust would require significant effort. SHACL-SPARQL constraints require a SPARQL engine, adding considerable complexity.

**Agent Ergonomics (3/5)**:
SHACL in Turtle syntax is moderately readable. Validation reports are structured RDF graphs with sh:result entries containing focus node, path, value, and message -- well-structured but verbose. AI agents can parse SHACL, but the RDF layer adds cognitive overhead compared to JSON Schema.

**Adoption/Community (3/5)**:
Growing adoption in knowledge graph and data governance communities. Used by TopBraid, Google Knowledge Graph, and EU data portals. Good tooling in Java (TopBraid SHACL, RDF4J). Limited tooling outside Java.

**Bridging Capability (2/5)**:
SHACL-to-JSON-Schema bridges exist (shacl2jsonschema) but are incomplete. No production bridges to SQL DDL, Protobuf, or TypeScript. The RDF foundation makes bridging to non-RDF formats inherently lossy.

**Verdict**: SHACL's validation model (severity levels, closed-world, structured reports) is the closest conceptual match to Axon's requirements among graph formats. However, the RDF dependency, lack of Rust tooling, and weak bridging capability make full adoption impractical. SHACL's design principles should influence ESF's validation model.

---

### 3.4 ShEx (Shape Expressions)

**What it is**: A concise, grammar-based language for describing and validating RDF graph structures. Developed as an alternative to SHACL with a more compact syntax. ShEx uses a notation similar to regular expressions applied to graph shapes.

**Entity-Graph Fitness (3/5)**:
Similar to SHACL -- shapes describe expected properties of nodes. ShEx supports recursive shapes, negation, and cardinality. Nested structures via shape references. Link typing via value constraints. Same RDF reification issues for link metadata.

**Validation Power (3/5)**:
Field-level constraints: datatype, cardinality, value sets, string patterns. ShEx lacks SHACL's SPARQL-based constraints, so complex cross-field validation is limited. No severity levels -- validation is pass/fail. ShEx supports closed shapes (CLOSED keyword), matching Axon's strict validation.

**Evolution Model (1/5)**:
No evolution support.

**Rust Ecosystem (2/5)**:
The `rudof` project (by Jose Emilio Labra Gayo, ShEx's co-creator) includes Rust implementations of both ShEx and SHACL. The `shex_validation` crate is more mature than the SHACL counterpart. However, it still depends on the full RDF stack and is not production-hardened for high-performance validation.

**Agent Ergonomics (3/5)**:
ShEx's compact syntax is more readable than SHACL:
```
:Person {
  :name xsd:string ;
  :age xsd:integer ? ;
  :knows @:Person *
}
```
This is concise and intuitive. However, the RDF namespace infrastructure adds noise.

**Adoption/Community (2/5)**:
Smaller community than SHACL. Strong academic backing but limited production adoption. The W3C never standardized ShEx (it is a community group spec, not a recommendation).

**Bridging Capability (2/5)**:
ShEx-to-JSON-Schema conversion is partially supported. Same RDF-to-non-RDF bridging challenges as SHACL.

**Verdict**: ShEx's compact syntax is appealing and its shape validation model is relevant to ESF design. But the RDF dependency, lack of severity levels, and limited ecosystem make it unsuitable as Axon's primary format.

---

### 3.5 JSON-LD

**What it is**: A JSON-based format for expressing linked data. Adds @context (namespace mapping), @type (RDF type), @id (IRI identifier), and @graph (named graph) to standard JSON. JSON-LD 1.1 is a W3C recommendation.

**Entity-Graph Fitness (3/5)**:
JSON-LD naturally represents entities as JSON objects with @type and @id. Links between entities are expressed as properties with @id references. Nested structures are native JSON. However, JSON-LD is a **serialization format**, not a schema language. It defines how data is structured, not what constraints apply. Link metadata can be expressed via @annotation (JSON-LD 1.1) but the semantics are limited.

**Validation Power (1/5)**:
JSON-LD itself has no validation capabilities. It must be paired with SHACL or ShEx for validation. The @context mapping adds semantic meaning but no constraints.

**Evolution Model (1/5)**:
No evolution model. Context documents can change, causing silent semantic shifts.

**Rust Ecosystem (3/5)**:
`json-ld` crate by timothee-haudebourg provides JSON-LD processing (expansion, compaction, flattening, framing). `sophia_rs` and `oxigraph` can consume JSON-LD as RDF. Reasonable Rust support for parsing but not for schema validation.

**Agent Ergonomics (4/5)**:
JSON-LD's key advantage: it is valid JSON. Any JSON parser can read it. AI agents handle JSON natively. The @context pattern is elegant for adding semantics without changing the JSON shape. Highly readable and writable.

**Adoption/Community (4/5)**:
Wide adoption: Schema.org uses JSON-LD for structured data on the web. Google Search consumes JSON-LD for rich results. W3C standard. Verifiable Credentials use JSON-LD. Strong ecosystem.

**Bridging Capability (2/5)**:
JSON-LD can round-trip to RDF, enabling access to the semantic web stack. But no direct bridges to SQL DDL, Protobuf, or TypeScript. It bridges to JSON Schema only in the sense that JSON-LD documents are JSON documents -- there is no semantic bridge.

**Verdict**: JSON-LD is a valuable serialization pattern (especially @context for semantic namespacing and @id/@type for entity identity) but is not a schema language. ESF could adopt JSON-LD conventions for entity serialization while using a separate mechanism for schema definition and validation.

---

### 3.6 PG-Schema (Property Graph Schema)

**What it is**: An emerging component of the ISO/IEC GQL (Graph Query Language) standard and the SQL/PGQ extension. PG-Schema defines schemas for property graphs: node types with property definitions, edge types with source/target constraints and property definitions. The ISO 9075 SQL:2023 standard includes SQL/PGQ, and GQL (ISO 39075) was published in 2024.

**Entity-Graph Fitness (5/5)**:
PG-Schema is the most natural fit for Axon's entity-graph model. Node types map to entity types; edge types map to link types with source/target constraints; properties on both nodes and edges are first-class. Cardinality constraints on edges (e.g., one-to-many) are part of the standard. This is the only format in this survey designed specifically for the property-graph model that Axon uses.

**Validation Power (3/5)**:
PG-Schema supports property type constraints (types, nullability, defaults) and edge endpoint constraints. However, it lacks complex validation rules: no cross-field constraints, no conditional validation, no severity levels, no regex patterns. It is a structural schema, not a validation framework.

**Evolution Model (2/5)**:
Being part of SQL/GQL, schema evolution follows SQL ALTER patterns. No declarative breaking-change detection. Migration is imperative (ALTER statements).

**Rust Ecosystem (1/5)**:
No Rust implementations of PG-Schema exist. The GQL standard is very new (2024) and implementations are nascent. Neo4j, Oracle, and TigerGraph are working on GQL support, but all in Java/C++. The `gqlparser` space in Rust is empty.

**Agent Ergonomics (3/5)**:
PG-Schema syntax (as specified in GQL) is SQL-like:
```sql
CREATE NODE TYPE Person (name STRING NOT NULL, age INT);
CREATE EDGE TYPE KNOWS (since DATE) FROM Person TO Person;
```
This is readable but unfamiliar to developers who have not used graph databases. Less ergonomic than JSON-based formats for AI agents.

**Adoption/Community (2/5)**:
Very early. The GQL standard was published in 2024 but production implementations are limited. Neo4j's GQL support is partial. The standard is 600+ pages and complex. Adoption will grow but is currently minimal.

**Bridging Capability (3/5)**:
Being part of the SQL family, SQL DDL generation is natural. Protobuf and TypeScript generation would need to be built. The SQL/PGQ integration suggests interop with relational systems, but this is implementation-dependent.

**Verdict**: PG-Schema's conceptual model is the best fit for Axon among all surveyed formats. The node-type/edge-type distinction maps directly to entity types and link types. However, zero Rust tooling, early standardization, and weak validation capabilities mean Axon cannot adopt PG-Schema directly. ESF should incorporate PG-Schema's design patterns (especially node/edge type definitions with endpoint constraints) while building Axon-specific validation.

---

## 4. Document/Object Schema Formats

### 4.1 JSON Schema (Draft 2020-12)

**What it is**: The dominant standard for describing and validating JSON data. Draft 2020-12 (latest stable) introduced $dynamicRef for recursive schemas, vocabulary extensibility, and output format specification. JSON Schema defines structure (type, properties, required, items) and validation (format, pattern, minimum/maximum, enum, const).

**Entity-Graph Fitness (3/5)**:
JSON Schema excels at describing nested document structures -- this is its core purpose. Objects, arrays, nested objects, recursive types ($dynamicRef or $ref with $defs) are all supported. However, JSON Schema has **no concept of links or relationships between documents**. A reference to another entity is just a string field with no semantic meaning. There is no way to express "this field is a typed link to an entity in collection X with cardinality 1:N." Custom extensions (x- keywords or vocabulary extensions) can add this, but it is not native.

**Validation Power (4/5)**:
Strong field-level validation: type, enum, const, pattern, format, minimum/maximum, minLength/maxLength, minItems/maxItems, uniqueItems, multipleOf. Cross-field: if/then/else, dependentRequired, dependentSchemas, allOf/anyOf/oneOf for composition. These enable moderately complex conditional validation (e.g., "if status is 'active', then end_date is required"). However, **cross-entity validation** is impossible (JSON Schema validates one document at a time). **Severity levels** do not exist -- validation is binary (valid/invalid). Context-specific constraints (per-LOB nullability) would require schema composition with conditional logic, which is verbose and fragile.

**Evolution Model (3/5)**:
JSON Schema itself has no evolution model, but its structure supports evolution analysis. Adding optional properties is always safe. Changing type, removing properties, or narrowing constraints is detectable by comparing schemas. The `json-schema-diff` ecosystem enables breaking-change detection. JSON Schema's additive nature (additional properties are allowed by default, can be restricted) supports gradual evolution. Draft 2020-12 vocabularies could define evolution semantics.

**Rust Ecosystem (5/5)**:
Best-in-class Rust support:
- **`jsonschema`** (crate by Dmitry Dygalo): Full Draft 2020-12 support, compiled validation (sub-microsecond for typical schemas), excellent error messages with JSON pointer paths. Actively maintained, 500+ GitHub stars. This is the leading JSON Schema validator in Rust.
- **`schemars`**: Derive JSON Schema from Rust types. Useful for generating schemas from Axon's internal types.
- **`boon`**: Alternative validator with good performance characteristics.
- **`referencing`**: $ref resolution for complex schema composition.

Performance: The `jsonschema` crate compiles schemas into an internal representation and validates documents in microseconds for typical schemas (<100 fields). Well within Axon's sub-millisecond requirement.

**Agent Ergonomics (5/5)**:
JSON Schema is the lingua franca of structured data for AI agents. Every major LLM provider uses JSON Schema for structured output (OpenAI function calling, Anthropic tool definitions, Google Gemini). Agents can read schemas, generate valid documents, and parse validation errors natively. Error messages from `jsonschema` include field path (JSON Pointer), expected constraint, and actual value.

**Adoption/Community (5/5)**:
Massive ecosystem. JSON Schema is used by OpenAPI/Swagger, AsyncAPI, MongoDB validation, AWS CloudFormation, Kubernetes CRDs, GitHub Actions, and countless APIs. Draft 2020-12 has broad tooling support across all major languages. Rich documentation at json-schema.org.

**Bridging Capability (4/5)**:
Strong bridging ecosystem:
- JSON Schema -> TypeScript: `json-schema-to-typescript` (mature, widely used)
- JSON Schema -> SQL DDL: Multiple tools, though lossy for complex schemas
- JSON Schema -> Protobuf: `jsonschema2protobuf` and similar (partial)
- JSON Schema -> Python/Go types: `datamodel-code-generator`, `go-jsonschema`
- Protobuf -> JSON Schema: `protoc-gen-jsonschema`
- SQL -> JSON Schema: Various tools extract schemas from DDL

**Verdict**: JSON Schema is the strongest candidate as ESF's foundation. Its Rust ecosystem is mature, its agent ergonomics are unmatched, and its bridging capabilities are broad. The critical gap is the complete absence of graph semantics (links, relationships, cardinality between entities). ESF would need to extend JSON Schema with a vocabulary for link types, cross-entity constraints, and severity levels.

---

### 4.2 TypeBox

**What it is**: A TypeScript library that creates JSON Schema-compliant types at runtime. TypeBox schemas ARE JSON Schemas (Draft 2020-12 compatible) but are constructed programmatically in TypeScript with full type inference.

**Entity-Graph Fitness (3/5)**:
Same as JSON Schema -- TypeBox produces JSON Schema, so it inherits all of JSON Schema's strengths and limitations. No link/relationship awareness.

**Validation Power (4/5)**:
Same as JSON Schema, with TypeBox-specific extensions (Transform types, custom formats). The TypeBox compiler can produce optimized validation functions.

**Evolution Model (3/5)**:
Same as JSON Schema.

**Rust Ecosystem (1/5)**:
TypeBox is TypeScript-only. The JSON Schema output can be consumed by Rust validators, but TypeBox itself has no Rust presence. It would only be relevant as a client-side SDK pattern.

**Agent Ergonomics (4/5)**:
TypeBox schemas are TypeScript code, which agents handle well. But the JSON Schema output is what matters for cross-language use.

**Adoption/Community (4/5)**:
Growing rapidly. Used by Elysia, Fastify (via `@fastify/type-provider-typebox`), and many TypeScript projects. The "schema as code" approach is popular.

**Bridging Capability (4/5)**:
Produces standard JSON Schema, so all JSON Schema bridges apply. Additionally, the TypeScript type inference means TypeScript types are generated automatically.

**Verdict**: TypeBox is relevant as a pattern for Axon's TypeScript client SDK -- providing TypeScript-first schema construction that compiles to ESF. Not relevant for the core schema format decision. Note: Axon's TypeScript SDK could offer a TypeBox-style builder that generates ESF schemas.

---

### 4.3 Zod / Valibot

**What it is**: TypeScript-first schema validation libraries. Zod is the most popular (30K+ GitHub stars); Valibot is a lightweight alternative (~6KB). Both define schemas as code with runtime validation, type inference, and transformation.

**Entity-Graph Fitness (2/5)**:
Schema-as-code with no standard serialization format. Zod schemas are TypeScript objects, not portable definitions. No link/relationship concepts.

**Validation Power (4/5)**:
Rich validation: types, refinements (custom predicates), transforms, pipes, discriminated unions, recursive types (z.lazy). Cross-field validation via .refine() and .superRefine(). However, all validation is runtime TypeScript -- no serializable schema.

**Evolution Model (1/5)**:
No evolution model. Schemas are code; changes are code changes.

**Rust Ecosystem (0/5)**:
No Rust presence. TypeScript-only by design.

**Agent Ergonomics (4/5)**:
For TypeScript agents, excellent. But not portable to other languages or contexts.

**Adoption/Community (5/5)**:
Zod is the dominant TypeScript validation library. Used by tRPC, React Hook Form, Astro, and many others. Valibot is growing as a lightweight alternative.

**Bridging Capability (3/5)**:
Zod-to-JSON-Schema (`zod-to-json-schema`) is well-maintained. This is the primary bridge path. No direct bridges to SQL, Protobuf, etc.

**Verdict**: Zod/Valibot are relevant as client-side validation patterns. Axon's TypeScript SDK could provide Zod-compatible schema definitions. Not relevant for the core ESF format.

---

### 4.4 Protocol Buffers (proto3)

**What it is**: Google's language-neutral, platform-neutral mechanism for serializing structured data. Proto3 defines message types with typed fields, enums, nested messages, oneof (union), maps, and well-known types (Timestamp, Duration, Any). Code generation produces serializers/deserializers and type definitions in 10+ languages.

**Entity-Graph Fitness (2/5)**:
Protobuf defines message structures with nested messages, which maps to entity field definitions. However, Protobuf has no relationship/link concept -- references are just integer/string fields. No cardinality between messages. No graph semantics. Deeply nested structures (8 levels) are supported but not idiomatic.

**Validation Power (2/5)**:
Proto3 has minimal validation: type checking and field presence (optional vs required was removed in proto3; all fields are optional by default). No pattern matching, no enums with associated data, no cross-field constraints, no conditional validation. `protoc-gen-validate` (PGV, now `protovalidate`/`buf validate`) adds field-level constraints via custom options:
```protobuf
string email = 1 [(buf.validate.field).string.email = true];
int32 age = 2 [(buf.validate.field).int32 = {gte: 0, lte: 150}];
```
But this is a third-party extension, not core Protobuf. No severity levels. No cross-entity validation.

**Evolution Model (4/5)**:
Protobuf has strong evolution rules: fields are numbered, adding fields is safe, removing fields requires reserving the number, type changes are restricted. The `buf` tool provides breaking-change detection (`buf breaking`). This is one of the best evolution models in the survey.

**Rust Ecosystem (4/5)**:
- **`prost`**: The standard Protobuf code generator for Rust. Generates Rust structs from .proto files. Used by tonic for gRPC.
- **`prost-reflect`**: Runtime reflection over Protobuf descriptors.
- **`protobuf`** (crate by stepancheg): Alternative implementation with reflection.
- **`buf`**: Linting and breaking-change detection (CLI tool, not Rust-native).

**Agent Ergonomics (3/5)**:
Proto syntax is readable but less familiar to web developers than JSON. Code generation means agents work with generated types, not the schema directly. Error messages from protobuf validation are structured but terse.

**Adoption/Community (5/5)**:
Massive. Used by Google, gRPC ecosystem, Kubernetes, Envoy, and thousands of services. Buf.build has modernized the tooling significantly.

**Bridging Capability (4/5)**:
- Protobuf -> JSON Schema: `protoc-gen-jsonschema`
- Protobuf -> TypeScript: `ts-proto`, `protobuf-ts`
- Protobuf -> SQL: Less common but possible via code generation
- Protobuf -> OpenAPI: `protoc-gen-openapiv2` (gRPC-Gateway)

**Verdict**: Protobuf is essential for Axon's gRPC API layer but unsuitable as the schema definition format. Its validation is too limited, it has no graph semantics, and its numbered-field model does not map to document schemas. ESF should generate Protobuf definitions as an export bridge, not use Protobuf as the source of truth.

---

### 4.5 Apache Avro

**What it is**: A data serialization system with rich schemas defined in JSON. Used extensively in Apache Kafka, Hadoop, and data pipeline ecosystems. Schemas define records with typed fields, unions, enums, fixed, and arrays. Avro is designed for schema evolution -- readers and writers can use different schema versions.

**Entity-Graph Fitness (2/5)**:
Avro records can be nested (records within records). Recursive types are supported via named types. But no link/relationship concept -- references are just fields. No graph semantics. Avro is designed for serialized data in motion (streams, files), not for entity-graph data at rest.

**Validation Power (2/5)**:
Avro validates type compatibility between reader and writer schemas. Field-level: types and defaults. No pattern matching, no value ranges, no cross-field constraints, no conditional validation. Validation is binary (compatible/incompatible).

**Evolution Model (5/5)**:
Avro's schema evolution is the gold standard for data pipelines. Schema Registry (Confluent) provides compatibility checking (BACKWARD, FORWARD, FULL, NONE). Rules: adding fields with defaults is safe, removing fields with defaults is safe, type promotions (int -> long) are safe. Breaking changes are detected automatically. This model directly informs how ESF should handle evolution.

**Rust Ecosystem (3/5)**:
- **`apache-avro`**: Official Apache Avro Rust implementation. Schema parsing, serialization/deserialization, schema evolution checking. Actively maintained.
- **`avro-rs`**: Earlier implementation, now deprecated in favor of `apache-avro`.

Performance is good for serialization but Avro is not designed for sub-millisecond document validation in the JSON Schema sense.

**Agent Ergonomics (3/5)**:
Avro schemas are JSON, which is agent-friendly. But Avro's union syntax (`["null", "string"]` for nullable strings) is unintuitive. Error messages are serialization-focused, not validation-focused.

**Adoption/Community (4/5)**:
Strong in data engineering: Kafka, Hadoop, Spark, Flink, dbt. Schema Registry is a mature tool. Weaker outside data pipelines.

**Bridging Capability (3/5)**:
Avro -> JSON Schema: Possible with some loss. Avro -> SQL DDL: Common in data pipeline tools. Avro -> Protobuf: Confluent Schema Registry supports dual registration. Limited TypeScript bridges.

**Verdict**: Avro's schema evolution model (compatibility modes, schema registry) is directly applicable to ESF's evolution strategy. Avro itself is too limited for entity-graph schemas, but its evolution patterns should be adopted.

---

### 4.6 Cap'n Proto / FlatBuffers

**What it is**: Zero-copy serialization formats. Cap'n Proto (by Kenton Varda, Protobuf v2 author) provides schemas with structs, lists, and unions. FlatBuffers (Google) provides similar capabilities. Both enable reading serialized data without deserialization.

**Entity-Graph Fitness (1/5)**:
Designed for efficient serialization, not schema definition. No link/relationship concepts. Nested structures are supported but limited by the zero-copy layout requirements.

**Validation Power (1/5)**:
Minimal validation -- type checking only. No constraints, patterns, or conditional logic.

**Evolution Model (3/5)**:
Both support additive evolution (new fields with defaults). Cap'n Proto's field numbering prevents reuse. FlatBuffers supports optional fields added at the end of tables.

**Rust Ecosystem (3/5)**:
- **`capnp`**: Official Cap'n Proto Rust library. Mature.
- **`flatbuffers`**: Official FlatBuffers Rust library. Mature.
Both are well-maintained but focused on serialization, not schema validation.

**Agent Ergonomics (2/5)**:
Schema IDL is readable but unfamiliar. Zero-copy access patterns are efficient but add complexity.

**Adoption/Community (3/5)**:
Cap'n Proto is niche but respected. FlatBuffers is used by Android (in some scenarios), gaming, and performance-critical applications. Neither has the ecosystem breadth of Protobuf.

**Bridging Capability (2/5)**:
Limited bridging. Some Cap'n Proto -> JSON Schema tools exist. No broad bridge ecosystem.

**Verdict**: Not relevant for ESF. Zero-copy formats may be useful for Axon's internal serialization (wire protocol, storage format) but not for schema definition.

---

## 5. Database-Native Schema Formats

### 5.1 EdgeDB SDL (Schema Definition Language)

**What it is**: EdgeDB's schema language for defining object types, links, computed properties, access policies, and constraints. EdgeDB is a "graph-relational" database built on PostgreSQL that uses SDL for schema definition and EdgeQL for queries.

**Entity-Graph Fitness (5/5)**:
EdgeDB SDL is the closest existing schema language to what Axon needs:
```sdl
type Person {
  required name: str;
  age: int32;
  multi friends: Person;  # typed link with cardinality
  multi authored: Document {
    # link properties (metadata on the link)
    role: str;
    since: datetime;
  };
}

type Document {
  required title: str;
  required content: str;
}
```
This natively expresses: entity types with nested properties, typed directional links between entities, link metadata (properties on links), cardinality (single/multi/required), and recursive types. The `multi`/`required`/`optional` modifiers express cardinality naturally. Abstract types enable inheritance. Computed properties provide derived fields.

**Validation Power (3/5)**:
EdgeDB supports constraints:
```sdl
type Person {
  required email: str {
    constraint exclusive;
    constraint regexp(r'^[^@]+@[^@]+$');
  }
  required age: int32 {
    constraint min_value(0);
    constraint max_value(150);
  }
}
```
Field-level constraints are solid (exclusive, regexp, min/max, expression). Cross-field constraints via `constraint expression on (...)` on the type. However: no severity levels (all constraints are errors), no conditional/contextual constraints, no cross-entity validation. The constraint language is less powerful than SHACL for complex validation scenarios.

**Evolution Model (3/5)**:
EdgeDB has migration support: `edgedb migration create` diffs the current schema against the new schema and generates DDL migration steps. Breaking changes require explicit migration. This is better than most alternatives but is tied to EdgeDB's migration engine.

**Rust Ecosystem (2/5)**:
EdgeDB has official Rust client libraries (`edgedb-tokio`, `edgedb-derive`) but these are for connecting to EdgeDB as a database, not for parsing/validating SDL independently. The SDL parser is in Rust (EdgeDB server is Rust+Python), but it is not published as a standalone crate. Extracting the SDL parser would require forking EdgeDB.

**Agent Ergonomics (5/5)**:
EdgeDB SDL is highly readable and writable. The syntax is intuitive, links are first-class, and the language is concise. AI agents can generate and parse EdgeDB SDL with minimal prompting. Error messages from EdgeDB are structured and specific.

**Adoption/Community (3/5)**:
EdgeDB has a dedicated community (10K+ GitHub stars on the server repo). However, it is a relatively niche database with lower adoption than Postgres, MongoDB, or Neo4j. The SDL is tightly coupled to EdgeDB -- no independent spec or tooling.

**Bridging Capability (3/5)**:
EdgeDB generates SQL DDL for its PostgreSQL backing store. TypeScript type generation is built-in (via `@edgedb/generate`). No Protobuf generation. No JSON Schema export. Bridges are EdgeDB-specific, not general-purpose.

**Verdict**: EdgeDB SDL is the most compelling design reference for ESF. Its link model with metadata, cardinality modifiers, and constraint syntax closely match Axon's requirements. However, adopting EdgeDB SDL directly is impractical: the parser is not standalone, the format is tied to EdgeDB's type system, and it lacks severity levels and conditional constraints. ESF should borrow heavily from EdgeDB SDL's syntax and design patterns while adding Axon-specific features.

---

### 5.2 TypeDB TypeQL DEFINE

**What it is**: TypeDB's schema language for defining entity types, relation types, and attribute types. TypeDB is a "polymorphic database" with a type system inspired by knowledge representation. DEFINE statements create the schema; inference rules enable reasoning.

**Entity-Graph Fitness (4/5)**:
TypeDB's model is entity-relation-attribute:
```typeql
define
  person sub entity,
    owns name,
    owns age,
    plays friendship:friend,
    plays authorship:author;

  document sub entity,
    owns title,
    plays authorship:document;

  friendship sub relation,
    relates friend;

  authorship sub relation,
    relates author,
    relates document,
    owns role,
    owns since;

  name sub attribute, value string;
  age sub attribute, value long;
```
Relations are first-class types with roles, which is more expressive than simple links. A relation can connect more than two entities (N-ary relationships). This is powerful but more complex than Axon's binary link model. TypeDB's attribute types are reusable across entity types (structural typing rather than nominal).

**Validation Power (3/5)**:
TypeDB validates type conformance, cardinality (via `@card` annotation), and structural rules. Inference rules enable derived facts. However, no regex constraints, no conditional validation, no severity levels, no cross-field predicates beyond type conformance.

**Evolution Model (2/5)**:
TypeDB supports additive schema changes (undefine to remove, define to add). No automatic breaking-change detection. Migration is manual.

**Rust Ecosystem (1/5)**:
TypeDB server is written in Rust (TypeDB 3.0), but the TypeQL parser is not published as a standalone crate. The Rust client driver (`typedb-driver`) connects to a running TypeDB instance. No way to use TypeQL for standalone schema definition and validation.

**Agent Ergonomics (4/5)**:
TypeQL DEFINE syntax is readable, with clear entity/relation/attribute distinctions. The roles in relations add expressiveness but also complexity. AI agents can generate TypeQL, though it requires understanding the entity-relation-attribute model.

**Adoption/Community (2/5)**:
TypeDB has a dedicated but small community (3K+ GitHub stars). Vaticle (the company) was acquired by Google DeepMind in 2024. The database is used in drug discovery, financial crime, and knowledge graph applications. Limited general adoption.

**Bridging Capability (1/5)**:
Minimal bridging. TypeDB is a self-contained system with limited export capabilities. No bridges to JSON Schema, SQL DDL, Protobuf, or TypeScript.

**Verdict**: TypeDB's N-ary relation model and role-based typing are intellectually interesting but more complex than Axon needs. The binary link model (source -> target) is sufficient for Axon's use cases. TypeDB's concept of relations as first-class types with their own attributes maps to Axon's link-type schemas with metadata. The Rust ecosystem limitation (no standalone parser) is a dealbreaker for adoption.

---

### 5.3 SurrealDB DEFINE

**What it is**: SurrealDB's schema definition language for multi-model (document, graph, key-value, relational) data. DEFINE statements create tables, fields, edges, indexes, and events.

**Entity-Graph Fitness (4/5)**:
SurrealDB supports typed edges (graph relationships) as first-class record types:
```surql
DEFINE TABLE person SCHEMAFULL;
DEFINE FIELD name ON person TYPE string;
DEFINE FIELD age ON person TYPE int;

DEFINE TABLE authored SCHEMAFULL TYPE RELATION FROM person TO document;
DEFINE FIELD role ON authored TYPE string;
DEFINE FIELD since ON authored TYPE datetime;

DEFINE TABLE document SCHEMAFULL;
DEFINE FIELD title ON document TYPE string;
```
The `TYPE RELATION FROM ... TO ...` syntax defines typed edges with source/target constraints and properties. This maps well to Axon's link model. Nested objects are supported via nested DEFINE FIELD statements.

**Validation Power (3/5)**:
Field-level assertions:
```surql
DEFINE FIELD email ON person TYPE string ASSERT string::is::email($value);
DEFINE FIELD age ON person TYPE int ASSERT $value >= 0 AND $value <= 150;
```
Assertions support arbitrary expressions including functions. However: no severity levels, no conditional/contextual constraints, no structured error output. Validation is binary (pass/fail).

**Evolution Model (2/5)**:
Schema changes are applied directly (DEFINE to add, REMOVE to delete). No automatic migration generation, no breaking-change detection.

**Rust Ecosystem (2/5)**:
SurrealDB is written in Rust, and the `surrealdb` crate provides an embeddable database. However, the schema definition language is not available as a standalone parser/validator. Using SurrealDB for schema validation means embedding the entire database. The `surrealdb-core` crate exists but is tightly coupled to the database engine.

**Agent Ergonomics (4/5)**:
SurrealQL syntax is SQL-like and readable. The DEFINE statement pattern is intuitive. AI agents handle SQL-like syntaxes well.

**Adoption/Community (3/5)**:
SurrealDB has significant hype (25K+ GitHub stars) but is early in production adoption. The company has faced stability and community trust issues. The multi-model approach is ambitious but unproven at scale.

**Bridging Capability (2/5)**:
Limited bridging. SurrealDB can import from SQL and export to JSON, but no systematic schema bridges to JSON Schema, Protobuf, or TypeScript.

**Verdict**: SurrealDB's DEFINE RELATION syntax is a good reference for ESF's link-type definitions. The assertion syntax for field-level validation is clean. However, the lack of standalone tooling and the database coupling make adoption impractical. ESF should reference SurrealDB's syntax for link-type and assertion patterns.

---

### 5.4 Prisma Schema Language

**What it is**: Prisma's schema language for defining database models, relations, enums, and field attributes. Prisma is an ORM/query builder for TypeScript/JavaScript and Rust (experimental).

**Entity-Graph Fitness (3/5)**:
Prisma models define entities with fields and relations:
```prisma
model Person {
  id        String   @id @default(uuid())
  name      String
  age       Int?
  documents Document[] @relation("authored")
}

model Document {
  id       String @id @default(uuid())
  title    String
  author   Person @relation("authored", fields: [authorId], references: [id])
  authorId String
}
```
Relations are expressed via @relation with explicit foreign keys. This is inherently relational -- relations are decomposed into foreign key fields, not modeled as first-class links. Many-to-many relations require explicit join models. Link metadata requires a separate model (no properties on relations). This is adequate for simple relationships but awkward for Axon's first-class link model.

**Validation Power (2/5)**:
Prisma validates types and relations. Field-level: types, optionality, defaults, @unique, @db-specific types. No pattern matching, no value ranges, no cross-field constraints, no conditional validation. Prisma's validation is structural, not semantic.

**Evolution Model (4/5)**:
Prisma Migrate generates SQL migrations from schema diffs. Breaking changes require explicit migration steps. The diffing is automatic and reliable. This is one of the better migration stories in the survey.

**Rust Ecosystem (2/5)**:
Prisma's query engine is written in Rust (`prisma-engines`), but the schema parser (`psl` - Prisma Schema Language) is an internal crate not designed for external use. The `prisma-client-rust` project provides a Rust client but depends on the Prisma engine. No standalone schema parser/validator for Rust consumers.

**Agent Ergonomics (4/5)**:
Prisma schema syntax is clean and widely known in the TypeScript ecosystem. AI agents generate Prisma schemas routinely. However, the relational decomposition (explicit foreign key fields) adds ceremony.

**Adoption/Community (5/5)**:
Prisma is the dominant TypeScript ORM (35K+ GitHub stars). Large ecosystem, excellent documentation, active community. The schema language is well-understood.

**Bridging Capability (4/5)**:
- Prisma -> SQL DDL: Core functionality (generates migrations for Postgres, MySQL, SQLite, SQL Server, MongoDB)
- Prisma -> TypeScript types: Built-in (Prisma Client generates types)
- Prisma -> JSON Schema: Community tools exist
- Prisma -> Protobuf: Not supported

**Verdict**: Prisma's migration generation and schema diffing are excellent references. But Prisma's relational model (foreign keys, join tables) is a poor fit for Axon's first-class link model. The schema language lacks the expressiveness needed for entity-graph schemas.

---

## 6. Data Pipeline Schema Formats

### 6.1 UMF (Universal Metadata Format)

**What it is**: The schema format from the tablespec project. YAML-based metadata definitions for data pipeline tables with per-LOB nullability, relationships with cardinality and confidence, validation rules with severity, derivation/survivorship strategies, and multi-format type mappings.

**Entity-Graph Fitness (2/5)**:
UMF is designed for flat or shallowly-nested table schemas, not entity graphs. Relationships are defined at the table level with cardinality and confidence, but they describe foreign-key-style joins, not typed links. No link metadata, no graph traversal semantics. The column-oriented model does not naturally express deep nesting (8 levels) or recursive types.

**Validation Power (5/5)**:
UMF's validation is the most sophisticated in this survey for data quality:
- Per-field validation rules with **severity levels** (error, warning, info) -- directly matching Axon's requirement
- **Context-specific constraints**: nullable per LOB (e.g., `nullable: {MD: false, ME: true}`) -- precisely the per-context constraint model Axon needs
- Validation rules with named checks, descriptions, and SQL/expression predicates
- Quality checks for post-write validation
- Domain types (email, phone_number) with semantic validation

This is the closest match to Axon's validation requirements. The gap is that UMF validation operates on rows/columns, not entity graphs.

**Evolution Model (2/5)**:
UMF tracks schema versions and supports some evolution metadata, but no automatic breaking-change detection or migration generation.

**Rust Ecosystem (1/5)**:
UMF is implemented in Python (tablespec). No Rust parser or validator exists. UMF schemas are YAML, which Rust can parse (via `serde_yaml`), but the UMF-specific semantics (type mappings, validation rules, derivation strategies) would need to be reimplemented.

**Agent Ergonomics (4/5)**:
YAML is readable and writable by AI agents. UMF schemas are self-documenting with descriptions, domain types, and named validation rules. The per-LOB nullability pattern is intuitive once understood.

**Adoption/Community (1/5)**:
Internal project. No external community. Documentation is limited to the tablespec repository.

**Bridging Capability (4/5)**:
UMF's core purpose is multi-format generation:
- UMF -> SQL DDL (multiple dialects)
- UMF -> JSON Schema
- UMF -> PySpark types
- UMF -> Great Expectations checks
- Extensible type mapping system

This bridging capability is directly relevant to ESF's bridging requirements.

**Verdict**: UMF contributes three critical concepts to ESF: (1) validation rules with severity levels, (2) context-specific constraints, and (3) multi-format type mapping and bridge architecture. UMF itself is too table-oriented for entity graphs, but its validation and bridging patterns should be directly incorporated into ESF. The technical requirements document already identifies these concepts for incorporation.

---

### 6.2 Great Expectations

**What it is**: A Python framework for data validation, documentation, and profiling. "Expectations" are declarative assertions about data (e.g., `expect_column_values_to_be_between`, `expect_column_pair_values_A_to_be_greater_than_B`). Expectations are grouped into "suites" and produce structured validation results.

**Entity-Graph Fitness (1/5)**:
Operates on tabular data (DataFrames, SQL tables). No entity or graph concepts. Expectations validate columns and rows, not nested structures or links.

**Validation Power (4/5)**:
Rich library of 50+ built-in expectations covering: column types, null rates, value ranges, uniqueness, regex patterns, set membership, cross-column comparisons, distribution checks, and custom expectations. Results include: success/failure per expectation, observed values, element counts, and partial unexpected value lists. Expectations support `mostly` parameter (e.g., 95% of values must match), which is a form of soft validation.

**Evolution Model (1/5)**:
No schema evolution. Expectations are versioned independently from data schemas.

**Rust Ecosystem (0/5)**:
Python-only. No Rust components.

**Agent Ergonomics (3/5)**:
Expectation results are structured JSON, which agents can parse. But the Python API is the primary interface.

**Adoption/Community (4/5)**:
Strong adoption in data engineering (15K+ GitHub stars). Used by data teams for pipeline validation. GX Cloud provides managed hosting. Active community.

**Bridging Capability (1/5)**:
No schema bridging. GX validates data against expectations but does not generate schemas.

**Verdict**: Great Expectations contributes the concept of declarative validation suites with structured results and soft thresholds (`mostly`). The "expectation" pattern (named, described, parameterized validation rules) is relevant to ESF's validation rule design. Not relevant as a schema format.

---

### 6.3 dbt schema.yml

**What it is**: dbt's YAML-based schema files define tests, documentation, and metadata for SQL models. Column-level tests include `not_null`, `unique`, `accepted_values`, `relationships` (referential integrity). Custom tests are SQL queries.

**Entity-Graph Fitness (1/5)**:
Designed for SQL models (tables/views). No entity or graph concepts. The `relationships` test checks foreign key integrity but is a data quality check, not a schema definition.

**Validation Power (3/5)**:
Built-in tests: not_null, unique, accepted_values, relationships. Custom tests via SQL. Results are pass/warn/fail with configurable severity. The severity model (`severity: warn` on individual tests) is relevant to ESF.

**Evolution Model (1/5)**:
No evolution model. schema.yml changes are unmanaged.

**Rust Ecosystem (0/5)**:
dbt is Python/Jinja2. No Rust components.

**Agent Ergonomics (4/5)**:
YAML syntax is clean and well-documented. AI agents generate dbt schema.yml routinely. The test/documentation pattern is intuitive.

**Adoption/Community (5/5)**:
dbt is the dominant transformation tool in modern data stacks (30K+ GitHub stars). schema.yml is ubiquitous in data engineering.

**Bridging Capability (1/5)**:
schema.yml generates SQL tests but no schema bridges.

**Verdict**: dbt contributes the pattern of YAML-based schema + validation definitions with per-test severity. The `severity: warn` pattern directly informs ESF's validation severity model. Not relevant as a schema format.

---

## 7. Deep Dive: Stardog

Stardog is the most instructive graph system for Axon's schema design because it solved a problem directly relevant to ESF: making semantic graph schemas practical enough for enterprise developers, not just ontology engineers. It also demonstrates both the power and the pitfalls of combining reasoning with validation.

### 7.1 Schema and Ontology Approach

Stardog's core insight is **dual-mode schema interpretation**: the same OWL ontology serves two purposes depending on context.

**Open World (Inference):** OWL axioms are interpreted under the Open World Assumption (OWA). If a class `Person` has a property restriction `hasEmail min 1`, and a person entity has no `hasEmail` triple, OWL reasoning concludes "we don't know their email" -- it does not flag an error. This mode is used for knowledge discovery, inference materialization, and ontology reasoning.

**Closed World (Validation):** The same OWL axioms -- or separate SHACL shapes -- are interpreted under the Closed World Assumption (CWA) for Integrity Constraint Validation (ICV). In CWA mode, that same `hasEmail min 1` restriction means "this person MUST have an email, and it's an error if they don't." ICV can run in "guard mode," rejecting transactions that violate constraints.

This split is achieved through configuration, not through writing constraints twice. The same ontology definition can serve both purposes depending on whether you are asking "what can we infer?" (OWA) or "is this data valid?" (CWA).

**What Axon should take from this:** Axon's ESF operates in closed-world mode only (schemas define what exists; extra fields are rejected). But the conceptual split is valuable -- Axon could later support "advisory" schema rules (warnings that don't reject writes, analogous to OWA interpretation) alongside "enforcement" rules (errors that reject writes, analogous to CWA). This maps directly to the severity-level requirement in the technical requirements (error/warning/info).

### 7.2 Virtual Graph Mapping Layer

Stardog's virtual graph capability allows SPARQL queries to transparently span:

- **Native RDF data** stored in Stardog
- **Relational databases** (PostgreSQL, MySQL, Oracle, SQL Server) mapped to RDF via R2RML or Stardog Mapping Syntax (SMS2)
- **Semi-structured sources** (JSON APIs, MongoDB, Elasticsearch, CSV files) mapped via SMS2
- **Other SPARQL endpoints** via federated query

The mapping language (SMS2) defines how source records map to RDF triples:

```
MAPPING
FROM SQL {
  SELECT id, name, email FROM customers
}
TO {
  ?customer a :Customer ;
    :name ?name ;
    :email ?email .
}
WHERE {
  BIND(template("http://example.org/customer/{id}") AS ?customer)
}
```

**What Axon should take from this:** Axon's "Schema Bridges" concept (ESF to JSON Schema, SQL DDL, Protobuf, TypeScript) is the right instinct but unidirectional. Stardog shows that bidirectional mapping -- querying external data through Axon's schema -- is powerful. For V1, Axon's bridges should be export-only. For P2, consider a virtual collection concept where external data sources appear as Axon collections with ESF schemas applied.

### 7.3 Data Model: Typed Properties, Cardinality, and Integrity

Stardog's ICV provides polyglot constraints: SHACL shapes, OWL axioms, SPARQL queries, or SWRL rules. All constraint types are internally translated to SPARQL queries for evaluation. ICV can run in three modes: on-demand validation, guard mode (reject violating transactions), and reporting (detailed SHACL validation report).

**What Axon should take from this:**
- The cardinality model maps directly to link-type definitions. ESF should support `minCount`/`maxCount` on link types.
- The polyglot constraint approach (SHACL + OWL + SPARQL) is too complex for Axon. ESF should have one constraint language, not three.
- Guard mode (reject on constraint violation) is exactly what Axon's "validate on write" does.

### 7.4 What Stardog Gets Right

1. **Making OWL useful for developers.** The lesson is not "use OWL" but "schema should do work beyond validation" -- it should drive query capabilities, API generation, and documentation.
2. **Closed-world validation on open-world data.** The dual CWA/OWA interpretation is genuinely clever. Axon's severity levels can achieve a similar effect without OWL's complexity.
3. **Virtual graphs eliminate ETL for read paths.** Worth studying for Axon's future virtual collections.
4. **Schema drives multiple query interfaces.** One ontology, three query languages. The schema is the single source of truth for what's queryable.
5. **ICV guard mode proves write-time validation works at enterprise scale.**

### 7.5 What Stardog Gets Wrong

1. **Enterprise pricing gates basic features.** Axon must keep schema enforcement in the open-source core, always.
2. **JVM dependency is a dealbreaker for embeddability.** Axon's Rust implementation avoids this entirely.
3. **Three constraint languages is two too many.** Most teams pick one and ignore the others.
4. **RDF as the user-facing data model alienates developers.** Axon's entity-graph-relational model is the right abstraction level.
5. **Reasoning at query time is unpredictable.** Axon is correct to defer inference to P2 (at earliest).

---

## 8. Deep Dive: TerminusDB

TerminusDB is relevant to Axon for three reasons: it treats schema as documents (JSON), it provides git-style versioning with diff/patch/merge, and it bridges between graph and document paradigms.

### 8.1 Schema as Documents

TerminusDB's schema evolution is instructive. Prior to version 10.0, schemas were defined in OWL -- which proved too complex for adoption. Since version 10, schemas are JSON documents with OWL semantics underneath:

```json
{
  "@type": "Class",
  "@id": "Customer",
  "name": "xsd:string",
  "email": "xsd:string",
  "status": {
    "@type": "Enum",
    "@id": "CustomerStatus",
    "@value": ["active", "inactive", "suspended"]
  },
  "addresses": {
    "@type": "Set",
    "@class": "Address"
  }
}
```

Key design decisions:
- **XSD datatypes** for scalar types (precise semantics but unfamiliar names).
- **Collection types** are explicit: Set (unordered, unique), List (ordered), Array (indexed). More precise than JSON Schema's single `array` type.
- **Multiple inheritance** is supported.
- **Closed-world assumption** by default. Extra properties not in the schema are rejected.

**What Axon should take from this:** The JSON-surface-over-rich-semantics pattern is exactly right for ESF. TerminusDB's migration from OWL to JSON validates that developer adoption requires JSON-native schemas.

### 8.2 Diff, Patch, and Merge for Data

TerminusDB's most distinctive feature is git-style version control for structured data. Every write operation creates an immutable commit. Diffs compare commits and return structured patches:

```json
{
  "op": "ModifyDocument",
  "path": ["Customer", "cust-123"],
  "operations": [
    {"op": "SwapValue", "path": ["status"], "before": "active", "after": "suspended"},
    {"op": "InsertValue", "path": ["suspended_at"], "value": "2026-04-04T12:00:00Z"}
  ]
}
```

Branches can be merged with conflict detection. Limitations: merge performance degrades when branch divergence exceeds approximately 20%.

**What Axon should take from this:**
- The structured diff format (path + operation + before/after) should inform Axon's audit entry structure.
- Data branching is harder than code branching. Storage design matters now; defer branching to P2/P3.

### 8.3 Graph-Native with Document Ergonomics

TerminusDB straddles the graph/document boundary: RDF triple store underneath, JSON document API on top. Developers interact with JSON in, JSON out; documents are decomposed into triples internally.

**What Axon should take from this:** The "documents on the surface, graph underneath" pattern validates Axon's entity-graph-relational approach. Entities look like JSON documents to API consumers. Links are first-class graph edges underneath.

---

## 9. Deep Dive: Cayley

Cayley is a Go-native graph database built on the quad-store model. It is relevant to Axon as a case study in embeddable graph databases with modular backends -- but also as a cautionary tale about schema minimalism.

### 9.1 Architecture

Cayley is modular at every layer. Multiple backend implementations (BoltDB, LevelDB, PostgreSQL, MongoDB, in-memory). Multiple query languages (Gizmo, MQL, GraphQL, SPARQL). A `schema` package maps Go structs to quads:

```go
type Person struct {
    ID      quad.IRI `quad:"@id"`
    Name    string   `quad:"name"`
    Friends []Person `quad:"friend"`
}
```

### 9.2 Schema: What's There and What's Missing

Cayley's schema package is explicitly **not** a schema enforcement system. No write-time validation, no type checking on quad values, no cardinality enforcement, no required-field enforcement. It is a serialization convenience, not a data integrity layer.

### 9.3 Key Lessons

1. **Modular backends prove the storage adapter pattern works.** Validates Axon's Storage Adapter trait design.
2. **Go struct mapping is excellent developer ergonomics.** Axon's client SDKs should provide similar struct/class mapping.
3. **Quad labels for partitioning are a good primitive.** Axon's collection model serves a similar purpose.
4. **No schema enforcement means no data integrity.** This is the exact problem Axon exists to solve.
5. **The storage adapter pattern is validated; schema enforcement is where the value lies.**

---

## 10. Schema Format Comparison Matrix

### 10.1 Type System Comparison

| Capability | JSON Schema | OWL 2 | SHACL | Protobuf | EdgeDB SDL | TypeQL | SurrealDB | Prisma |
|------------|:-----------:|:-----:|:-----:|:--------:|:----------:|:------:|:---------:|:------:|
| Scalar types | Yes | Via XSD | Via XSD | Yes | Yes | Yes | Yes | Yes |
| Enums | Yes | Via oneOf | Yes | Yes | Yes | No (subtypes) | Yes | Yes |
| Nested objects | Yes | Via restrictions | Via shapes | Yes (messages) | Yes | Via relations | Yes | Yes |
| Arrays/lists | Yes | Via collections | Via sh:list | Yes (repeated) | Yes (multi) | Via has | Yes | Yes |
| Maps/dictionaries | Yes | No | No | Yes (map) | No | No | No | No |
| Optional/required | Yes | Via cardinality | Yes | Yes (optional) | Yes (required) | Via constraints | Yes | Yes (?) |
| Default values | Yes | No | Yes | Yes | Yes | No | Yes | Yes |
| Recursive types | Yes ($ref) | Yes | Yes | Yes | Yes | Yes | Partial | Yes |
| Union types | Yes (oneOf) | Yes | Yes (sh:or) | Yes (oneof) | Abstract types | No | No | No |
| Cross-field constraints | Partial (if/then) | Via restrictions | Yes (SPARQL) | No | Yes | Yes (rules) | Yes (ASSERT) | No |
| Semantic types | Yes (format) | Custom datatypes | Via patterns | No | Custom scalars | No | Via functions | No |

### 10.2 Relationship/Link Schema Comparison

| Capability | JSON Schema | SHACL | EdgeDB SDL | TypeQL | SurrealDB | Prisma | PG-Schema |
|------------|:-----------:|:-----:|:----------:|:------:|:---------:|:------:|:---------:|
| Typed relationships | No ($ref only) | Yes | Yes (link) | Yes (relates) | Yes (RELATION) | Yes (@relation) | Yes (EDGE TYPE) |
| Relationship metadata | No | Qualified shapes | Link properties | Relation attributes | Edge fields | Separate model | Edge properties |
| Cardinality | No | sh:minCount/maxCount | single/multi/required | Via rules | No | Implicit | Yes |
| Directionality | N/A | sh:path | Source -> target | Role players | FROM -> TO | fields/references | FROM -> TO |
| Inverse relationships | N/A | Partial | Backlinks | Roles | No | @relation | No |
| Cross-collection | Via $ref | Any shape | Cross-type | Cross-entity | Cross-table | Cross-model | Cross-type |

### 10.3 Validation Model Comparison

| Capability | JSON Schema | SHACL | EdgeDB | SurrealDB | UMF | GX | dbt |
|------------|:-----------:|:-----:|:------:|:---------:|:---:|:--:|:---:|
| Validate on write | App-level | App/endpoint | Yes | Yes | Post-write | Post-write | Post-build |
| Multiple violations reported | Yes | Yes | Yes | Partial | Yes | Yes | Yes |
| Structured error messages | Library-dependent | Yes (RDF report) | Yes | Partial | Yes | Yes (JSON) | Yes |
| Severity levels | No | Yes (3 levels) | No | No | Yes (3 levels) | Partial (mostly) | Yes (warn/fail) |
| Conditional constraints | Yes (if/then) | Yes (SPARQL) | Yes (computed) | Yes (ASSERT) | Yes (per-context) | No | Custom SQL |
| Cross-entity constraints | No | Yes | Partial | No | Via SQL | Via SQL | Via SQL |

---

## 11. Scoring Summary

| Format | Entity-Graph | Validation | Evolution | Rust | Agent | Adoption | Bridging | **Total** |
|--------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| **OWL** | 3 | 2 | 2 | 1 | 1 | 3 | 2 | **14** |
| **RDFS** | 2 | 1 | 1 | 2 | 2 | 3 | 1 | **12** |
| **SHACL** | 3 | 4 | 2 | 2 | 3 | 3 | 2 | **19** |
| **ShEx** | 3 | 3 | 1 | 2 | 3 | 2 | 2 | **16** |
| **JSON-LD** | 3 | 1 | 1 | 3 | 4 | 4 | 2 | **18** |
| **PG-Schema** | 5 | 3 | 2 | 1 | 3 | 2 | 3 | **19** |
| **JSON Schema** | 3 | 4 | 3 | 5 | 5 | 5 | 4 | **29** |
| **TypeBox** | 3 | 4 | 3 | 1 | 4 | 4 | 4 | **23** |
| **Zod/Valibot** | 2 | 4 | 1 | 0 | 4 | 5 | 3 | **19** |
| **Protobuf** | 2 | 2 | 4 | 4 | 3 | 5 | 4 | **24** |
| **Avro** | 2 | 2 | 5 | 3 | 3 | 4 | 3 | **22** |
| **Cap'n Proto** | 1 | 1 | 3 | 3 | 2 | 3 | 2 | **15** |
| **EdgeDB SDL** | 5 | 3 | 3 | 2 | 5 | 3 | 3 | **24** |
| **TypeDB TypeQL** | 4 | 3 | 2 | 1 | 4 | 2 | 1 | **17** |
| **SurrealDB DEFINE** | 4 | 3 | 2 | 2 | 4 | 3 | 2 | **20** |
| **Prisma Schema** | 3 | 2 | 4 | 2 | 4 | 5 | 4 | **24** |
| **UMF** | 2 | 5 | 2 | 1 | 4 | 1 | 4 | **19** |
| **Great Expectations** | 1 | 4 | 1 | 0 | 3 | 4 | 1 | **14** |
| **dbt schema.yml** | 1 | 3 | 1 | 0 | 4 | 5 | 1 | **15** |

### Key Insight

No format scores above 29/35. JSON Schema leads due to its Rust ecosystem, agent ergonomics, and adoption, but has a critical gap in entity-graph fitness (3/5). EdgeDB SDL and PG-Schema have the best entity-graph fitness (5/5) but lack Rust tooling and validation features. UMF has the best validation model (5/5) but is table-oriented with no entity-graph support.

**The optimal design combines**: JSON Schema's ecosystem and agent ergonomics + EdgeDB SDL's link model + UMF's validation semantics + Avro's evolution model + PG-Schema's conceptual framing.

---

## 12. Graph System Schema Capabilities

| Capability | Stardog | TerminusDB | Cayley | Neo4j 5.x | TypeDB | **Axon (Target)** |
|------------|:-------:|:----------:|:------:|:---------:|:------:|:-----------------:|
| Schema required | No (ICV available) | Yes | No | No (opt-in) | Yes | **Yes** |
| Write-time validation | Yes (guard mode) | Yes | No | Partial | Yes | **Yes** |
| Typed entity properties | Yes (XSD) | Yes (XSD) | No | Partial (5.x) | Yes | **Yes (JSON Schema types + extensions)** |
| Typed relationships | Yes (ObjectProperty) | Yes (class links) | No | Yes (rel types) | Yes (relation types) | **Yes (link types)** |
| Relationship metadata | Via reification | No native | No | Yes (rel props) | Yes (rel attributes) | **Yes (link metadata schema)** |
| Cardinality constraints | Yes (OWL + SHACL) | Yes | No | No | Via rules | **Yes (minCount/maxCount)** |
| Cross-entity constraints | Yes (ICV) | Yes | No | No | Yes (rules) | **P1** |
| Schema versioning | Manual | Git-style commits | N/A | Manual | Manual | **Monotonic version per collection** |
| Structured errors | Yes (ICV report) | Yes | N/A | Basic | Yes | **Yes (path, expected, actual, suggestion)** |
| Severity levels | No native | No | N/A | N/A | No | **Yes (error/warning/info)** |
| Schema-driven query gen | Yes (ontology -> GraphQL) | No | No | No | Partial | **Yes (ESF -> query API)** |
| Embeddable | No (JVM) | Partial (heavy) | Yes (Go) | JVM only | No (JVM) | **Yes (Rust, in-process)** |
| ACID transactions | Yes | Yes | No | Yes | Yes | **Yes** |
| Audit trail | No | Git-style commits | No | No | No | **Yes (per-mutation)** |

---

## 13. ADR Option Analysis

### Option A: Adopt JSON Schema with Graph Extensions

**Approach**: Use JSON Schema Draft 2020-12 as the core format. Define a custom JSON Schema vocabulary (per Draft 2020-12 spec) that adds: link-type definitions, cardinality constraints, cross-entity references, severity levels on validation rules, and context-specific constraints.

**Example ESF schema (YAML representation of JSON Schema + extensions)**:
```yaml
$schema: "https://axon.dev/esf/2026-04/schema"
$id: "https://example.com/schemas/person"
type: object
x-axon-entity:
  collection: people
  link-types:
    authored:
      target: { collection: documents }
      cardinality: "1:N"
      metadata:
        type: object
        properties:
          role: { type: string, enum: [author, editor, reviewer] }
          since: { type: string, format: date-time }
    knows:
      target: { collection: people }
      cardinality: "N:M"
properties:
  name:
    type: string
    minLength: 1
  email:
    type: string
    format: email
    x-axon-validation:
      - rule: unique-within-collection
        severity: error
  age:
    type: integer
    minimum: 0
    maximum: 150
    x-axon-validation:
      - rule: range-check
        severity: warning
        message: "Age outside typical range"
        contexts:
          production: { severity: error }
          staging: { severity: warning }
required: [name, email]
```

**Pros**:
- Leverages the `jsonschema` Rust crate for field-level validation (sub-microsecond performance)
- JSON Schema is the lingua franca for AI agents -- zero learning curve
- Massive existing tooling ecosystem for bridging (TypeScript, SQL, OpenAPI)
- Custom vocabulary is the endorsed extension mechanism in Draft 2020-12
- YAML/JSON input natively supported
- Incremental adoption: start with standard JSON Schema, add extensions as needed

**Cons**:
- `x-axon-*` extensions are not validated by standard JSON Schema validators -- Axon must implement custom validation for all extension keywords
- Link-type definitions are bolted on, not first-class -- they feel foreign in JSON Schema
- Cross-entity validation requires a validation layer above JSON Schema
- Schema evolution checking must be built separately (JSON Schema has no built-in evolution model)
- Recursive types via $dynamicRef are complex and poorly understood

**Effort estimate**: Medium. JSON Schema parsing and field-level validation are free (via `jsonschema` crate). Link-type validation, cross-entity constraints, severity levels, and context-specific rules require custom implementation on top.

---

### Option B: Adopt SHACL/ShEx for Graph-Native Validation

**Approach**: Use SHACL as the schema and validation language. Entities are RDF nodes; links are RDF properties. Shapes define entity and link constraints.

**Pros**:
- Native graph validation with closed-world assumption
- Severity levels (sh:Violation, sh:Warning, sh:Info) built in
- Structured validation reports
- W3C standard with formal semantics

**Cons**:
- No production SHACL validator in Rust -- would need to build one
- RDF dependency adds massive conceptual overhead (IRIs, blank nodes, named graphs)
- Agent ergonomics are poor -- developers must think in RDF triples
- Bridging to JSON Schema, SQL, Protobuf is lossy and undertooled
- SPARQL-based constraints require embedding a SPARQL engine
- Deeply nested structures are unnatural in RDF's flat triple model

**Effort estimate**: Very high. Building a SHACL validator in Rust is a multi-month effort. The RDF dependency infects the entire stack.

**Recommendation**: Reject. The RDF tax is too high. SHACL's validation semantics (severity levels, structured reports) should be borrowed, but the RDF substrate should not.

---

### Option C: Adopt EdgeDB SDL (or Similar Graph-Relational SDL)

**Approach**: Define ESF as a syntactic variant of EdgeDB SDL. Parse it into an AST and implement validation against that AST.

**Pros**:
- First-class links with metadata -- the most natural syntax for Axon's model
- Readable, concise, agent-friendly
- Proven design (EdgeDB has shipped this and refined it over years)
- Computed properties and access policies are useful patterns

**Cons**:
- EdgeDB's SDL parser is not available as a standalone crate
- Would need to write a parser from scratch (or fork EdgeDB's)
- Loses JSON Schema ecosystem entirely -- no `jsonschema` crate, no OpenAPI compatibility
- Custom SDL means custom tooling for everything: editor support, linting, formatting, documentation generation
- AI agents are less familiar with EdgeDB SDL than with JSON Schema
- Bridging FROM EdgeDB SDL to JSON Schema is extra work (reverse direction from Option A)

**Effort estimate**: High. Custom parser, custom validator, custom tooling. The parser alone (with good error messages) is significant work.

**Recommendation**: Borrow the design patterns aggressively, but do not adopt the format. The ecosystem cost of a custom SDL is too high for a new project.

---

### Option D: Define Custom ESF (Entity Schema Format) with Bridges

**Approach**: Design ESF as a standalone YAML/JSON format purpose-built for Axon's entity-graph model. Not based on JSON Schema. Custom parser, custom validator, custom bridges.

**Pros**:
- Complete control over every aspect of the format
- Can be optimized for Axon's exact requirements
- No inherited limitations from any existing format

**Cons**:
- Maximum implementation effort -- everything is built from scratch
- No ecosystem leverage -- every tool, bridge, and integration is custom
- AI agents must learn a novel format
- Validation of the format itself needs a meta-schema
- Risk of designing a worse version of something that already exists

**Effort estimate**: Very high. And ongoing -- every bridge, tool, and integration is Axon's responsibility forever.

**Recommendation**: Reject. The ecosystem cost is prohibitive for a new project. This option makes sense only if no existing format can serve as a foundation, which is not the case.

---

### Option E: Hybrid -- JSON Schema for Entity Bodies + Custom Link/Validation Layer

**Approach**: Use standard JSON Schema Draft 2020-12 for entity field definitions (the "body" of an entity). Define a separate, Axon-specific schema layer for: link-type definitions, cross-entity constraints, validation severity, context-specific rules, and evolution metadata. The two layers are composed in a single schema document.

**Example ESF schema**:
```yaml
# ESF document -- two layers composed
esf: "1.0"
entity:
  collection: people
  # Standard JSON Schema for the entity body
  schema:
    $schema: "https://json-schema.org/draft/2020-12/schema"
    type: object
    properties:
      name: { type: string, minLength: 1 }
      email: { type: string, format: email }
      age: { type: integer, minimum: 0, maximum: 150 }
      address:
        type: object
        properties:
          street: { type: string }
          city: { type: string }
          state: { type: string, pattern: "^[A-Z]{2}$" }
        required: [street, city, state]
    required: [name, email]
    additionalProperties: false
    $defs:
      tree_node:
        type: object
        properties:
          value: { type: string }
          children:
            type: array
            items: { $ref: "#/$defs/tree_node" }

  # Axon link-type definitions (inspired by EdgeDB SDL / PG-Schema)
  links:
    authored:
      target: documents
      cardinality: "1:N"
      required: false
      metadata:
        $schema: "https://json-schema.org/draft/2020-12/schema"
        type: object
        properties:
          role: { type: string, enum: [author, editor, reviewer] }
          since: { type: string, format: date-time }
        required: [role]
    knows:
      target: people
      cardinality: "N:M"

  # Axon validation rules (inspired by UMF / SHACL)
  validations:
    - name: email-uniqueness
      severity: error
      scope: collection
      rule: "unique($.email)"
      message: "Email must be unique within the people collection"

    - name: age-range-warning
      severity: warning
      scope: entity
      rule: "$.age >= 13 AND $.age <= 120"
      message: "Age outside typical operating range"
      contexts:
        production:
          severity: error
          message: "Age must be between 13 and 120 in production"
        staging:
          severity: warning

    - name: address-completeness
      severity: info
      scope: entity
      rule: "EXISTS($.address) IMPLIES EXISTS($.address.zip)"
      message: "Consider adding zip code when address is provided"

  # Evolution metadata (inspired by Avro / Protobuf)
  evolution:
    version: 3
    compatibility: backward  # backward | forward | full | none
    history:
      - version: 1
        changes: [{ op: create }]
      - version: 2
        changes: [{ op: add, path: "/properties/address" }]
      - version: 3
        changes: [{ op: add, path: "/properties/age" }]
```

**How validation works**:
1. **Layer 1 -- JSON Schema**: The `schema` block is validated by the `jsonschema` crate. Sub-microsecond. Handles: types, required fields, patterns, enums, nested structure, recursive types, conditional schemas (if/then/else).
2. **Layer 2 -- Link validation**: Axon validates link operations against link-type definitions. Checks: target collection exists, cardinality constraints, link metadata validates against the metadata schema (which is also JSON Schema).
3. **Layer 3 -- Axon validation rules**: Custom validation engine evaluates rules with severity levels and context-specific overrides. Handles: cross-field constraints, collection-level uniqueness, conditional severity, informational checks.

**Pros**:
- Entity body validation is free via `jsonschema` crate (sub-microsecond, production-proven)
- JSON Schema portion is standard -- all existing tooling works (TypeScript generation, OpenAPI, editor support)
- Link definitions are clean and purpose-built (no awkward JSON Schema extensions)
- Validation rules with severity/contexts directly implement UMF patterns
- Evolution model draws from Avro's compatibility modes
- YAML format is agent-readable and writable
- Bridging strategy: JSON Schema body bridges to TypeScript/SQL/Protobuf natively; link definitions bridge via custom generators
- Clear separation of concerns -- each layer does what it is good at

**Cons**:
- Two conceptual layers adds complexity (developers learn JSON Schema + Axon-specific extensions)
- Validation rules use a custom expression language (must be designed and implemented)
- Link definitions are Axon-specific (no external tooling)
- Meta-validation (validating the ESF document itself) requires a custom validator

**Effort estimate**: Medium. JSON Schema validation is free. Link validation is straightforward. The custom validation rule engine is the main effort, but can start simple (field existence, comparisons) and grow.

---

## 14. Draft ADR Recommendation

### Recommended Option: E -- Hybrid (JSON Schema + Custom Link/Validation Layer)

**Rationale**:

1. **Pragmatism over purity**: JSON Schema gives Axon the best Rust ecosystem (`jsonschema` crate), the best agent ergonomics (every LLM knows JSON Schema), and the broadest bridging capability -- for free. Building a custom format for entity body validation when a production-proven, sub-microsecond validator exists is unjustifiable.

2. **Graph semantics deserve first-class treatment**: Bolting link types onto JSON Schema via `x-` extensions (Option A) produces an uncanny valley -- it looks like JSON Schema but is not JSON Schema. A separate, purpose-built link definition layer is clearer and more honest. Developers know which parts are standard JSON Schema and which parts are Axon-specific.

3. **Validation power from UMF lineage**: The validation rules layer directly inherits UMF's severity levels and context-specific constraints -- the two features that are absent from every other format surveyed. This is Axon's differentiation in the schema space.

4. **Evolution model from Avro/Protobuf best practices**: Axon does not need to invent evolution semantics. Avro's compatibility modes (backward, forward, full, none) and Protobuf's breaking-change detection (`buf breaking`) provide proven patterns to implement.

5. **Incremental complexity**: V1 can ship with just the JSON Schema layer (entity body validation) and link-type definitions. Validation rules (Layer 3) and evolution checking can be added in P1. Each layer is independently useful.

### Design Influences by Source

| ESF Aspect | Primary Influence | What We Take |
|-----------|-------------------|-------------|
| Entity body schema | **JSON Schema Draft 2020-12** | Type system, field constraints, $ref for composition, if/then/else for conditionals |
| Link-type definitions | **EdgeDB SDL** + **PG-Schema** | Typed links with target collection, cardinality (single/multi), metadata schema |
| Link metadata schema | **JSON Schema** | Link metadata validated as JSON Schema (reusing the same validator) |
| Validation rules | **UMF (tablespec)** + **SHACL** | Named rules with severity (error/warning/info), context-specific overrides, structured results |
| Schema evolution | **Apache Avro** + **Protobuf/buf** | Compatibility modes (backward/forward/full/none), automatic breaking-change detection |
| Schema-as-data | **TerminusDB** | Schemas stored as versioned, queryable, auditable entities |
| Constraint vocabulary | **Stardog ICV** (lesson: use ONE) | Single expression language for all Axon-specific constraints |
| Structured errors | **SHACL validation reports** | Validation results with focus path, expected value, actual value, severity, and actionable message |
| Bridge architecture | **UMF type mappings** | Extensible bridge system: ESF -> JSON Schema, SQL DDL, Protobuf, TypeScript |
| Query generation | **Stardog schema-driven APIs** | ESF definitions drive query API capabilities and client SDK types |

### Implementation Sequence

| Phase | Scope | Key Crates/Tools |
|-------|-------|-----------------|
| V1 P0 | JSON Schema entity validation + link-type definitions | `jsonschema`, `serde_yaml`, `serde_json` |
| V1 P0 | Structured validation errors (field path, expected, actual, message) | Custom error types on top of `jsonschema` output |
| V1 P1 | Validation rules with severity levels | Custom expression evaluator (start with simple predicates) |
| V1 P1 | Schema evolution: additive-safe detection, version tracking | Schema comparison logic (diff two JSON Schemas) |
| V1 P1 | Context-specific constraints | Configuration layer on validation rules |
| P2 | ESF -> SQL DDL bridge | Custom generator |
| P2 | ESF -> Protobuf bridge | Custom generator (or leverage `prost-reflect`) |
| P2 | ESF -> TypeScript bridge | Leverage `json-schema-to-typescript` for entity body; custom for links |
| P2 | ESF <-> UMF bridge | Bidirectional converter for data pipeline integration |

### Key Design Decisions for ESF ADR

1. **ESF documents are YAML** (with JSON as an alternative serialization). YAML is more readable for schema definitions; JSON is more portable for programmatic use.
2. **Entity body schemas are valid JSON Schema Draft 2020-12**. Any compliant JSON Schema validator can validate the entity body portion independently.
3. **Link-type definitions use an Axon-specific format** inspired by EdgeDB SDL's link model and PG-Schema's edge-type definitions.
4. **Validation rules use an Axon-specific expression language** with severity levels (error/warning/info) and context-specific overrides, inspired by UMF.
5. **Schema evolution uses Avro-style compatibility modes** (backward, forward, full, none) with automatic breaking-change detection.
6. **The `jsonschema` crate is the validation engine** for entity body validation. Axon does not reimplement JSON Schema validation.

### Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Custom expression language for validation rules becomes complex | Start with a minimal predicate set (field existence, comparison, regex). Use JSONPath for field references. Do not build a general-purpose language |
| Two-layer model confuses developers | Clear documentation. The JSON Schema layer is "what a document looks like" and the Axon layer is "how documents relate and what additional rules apply" |
| JSON Schema limitations force workarounds | Monitor which constraints developers need that JSON Schema cannot express. These become validation rules in Layer 3 |
| Schema evolution detection is hard to get right | Start with structural comparison (JSON Schema diff). Flag all non-additive changes as potentially breaking. Refine heuristics over time |

---

## 15. Recommended ESF Architecture

```
                    +-----------------------+
                    |    ESF Definition      |
                    |  (JSON Schema 2020-12  |
                    |   + Axon vocabulary)   |
                    +-----------+-----------+
                                |
            +-------------------+-------------------+
            |                   |                   |
    +-------v-------+  +-------v-------+  +-------v-------+
    | Validation     |  | Query API     |  | Schema Bridges |
    | Engine         |  | Generation    |  |               |
    | (write-time)   |  | (from ESF)    |  | ESF -> JSON   |
    |                |  |               |  | ESF -> SQL DDL|
    | - type checks  |  | - filter ops  |  | ESF -> Proto  |
    | - cardinality  |  | - sort fields |  | ESF -> TS     |
    | - link types   |  | - link trav.  |  | ESF <-> UMF   |
    | - severity     |  | - aggregation |  |               |
    +-------+-------+  +-------+-------+  +-------+-------+
            |                   |                   |
    +-------v-------------------v-------------------v-------+
    |                   Storage Adapter                      |
    |  (SQLite / PostgreSQL / FoundationDB)                  |
    +-------------------------------------------------------+
```

ESF is the single source of truth. Everything else -- validation, queries, SDK types, backing store DDL -- is derived from it.

---

## 16. Key Takeaways

### From Graph Systems (Stardog, TerminusDB, Cayley)

1. **Schema should do work beyond validation.** ESF definitions should drive query API generation, client SDK types, documentation, and backing store DDL -- not just write-time validation.
2. **Closed-world validation is the right default.** Guard mode (reject invalid writes) is proven in production at Stardog.
3. **One constraint language, not three.** Stardog's polyglot constraints create confusion. ESF uses one vocabulary.
4. **JSON surface over rich semantics wins adoption.** TerminusDB's migration from OWL to JSON is proof. ESF must be JSON-native.
5. **Schema-as-data enables powerful operations.** Storing schemas as versioned, queryable entities means schema changes get the same audit trail as data changes.
6. **The storage adapter pattern is validated.** Multiple backends behind one API works.

### Universal Lesson

Every system that started with academic schema formats (OWL, RDF Schema) later added JSON-based alternatives (TerminusDB v10, Stardog GraphQL, JSON-LD) or lost developer adoption entirely. ESF must be JSON-first from day one, with semantic richness expressed through JSON Schema extensions rather than imported formalisms.

---

## Appendix A: Rust Crate Reference

| Crate | Purpose | Maturity | Notes |
|-------|---------|----------|-------|
| `jsonschema` | JSON Schema Draft 2020-12 validation | Production | Sub-microsecond validation. Best-in-class |
| `schemars` | Generate JSON Schema from Rust types | Production | Useful for Axon internal type documentation |
| `serde_json` | JSON serialization/deserialization | Production | Foundational |
| `serde_yaml` | YAML serialization/deserialization | Production | For YAML schema input |
| `prost` | Protobuf code generation | Production | For gRPC API definitions |
| `prost-reflect` | Protobuf runtime reflection | Stable | For ESF -> Protobuf bridge |
| `apache-avro` | Avro schema parsing and evolution | Stable | Reference for evolution model implementation |
| `json-ld` | JSON-LD processing | Stable | If JSON-LD serialization is needed |
| `sophia_rs` | RDF graph library | Stable | If RDF interop is needed (unlikely in V1) |
| `oxigraph` | RDF store with SPARQL | Stable | If SPARQL is needed (unlikely) |
| `rudof` | ShEx/SHACL validation | Experimental | Reference only -- not production-ready |
| `boon` | Alternative JSON Schema validator | Stable | Backup to `jsonschema` crate |
| `horned-owl` | OWL 2 parsing | Stable | Reference only |
| `capnp` | Cap'n Proto serialization | Production | If zero-copy wire format is needed |
| `flatbuffers` | FlatBuffers serialization | Production | If zero-copy wire format is needed |

---

## Appendix B: Sources

### Specifications and Standards
- [JSON Schema Draft 2020-12 Specification](https://json-schema.org/draft/2020-12/json-schema-core)
- [JSON Schema Vocabulary System](https://json-schema.org/draft/2020-12/json-schema-core#section-8.1)
- [SHACL W3C Recommendation](https://www.w3.org/TR/shacl/)
- [ShEx Specification](http://shex.io/shex-semantics/)
- [OWL 2 Web Ontology Language](https://www.w3.org/TR/owl2-overview/)
- [JSON-LD 1.1](https://www.w3.org/TR/json-ld11/)
- [GQL/PG-Schema ISO 39075](https://www.iso.org/standard/76120.html)
- [Apache Avro Schema Evolution](https://avro.apache.org/docs/current/specification/)
- [Protocol Buffers Language Guide (proto3)](https://protobuf.dev/programming-guides/proto3/)

### Database Documentation
- [EdgeDB SDL Documentation](https://www.edgedb.com/docs/datamodel/index)
- [TypeDB TypeQL DEFINE](https://typedb.com/docs/typeql/statements/define)
- [SurrealDB DEFINE Statement](https://surrealdb.com/docs/surrealql/statements/define)
- [Prisma Schema Reference](https://www.prisma.io/docs/orm/reference/prisma-schema-reference)

### Graph Systems
- [Stardog Data Quality Constraints (ICV)](https://docs.stardog.com/data-quality-constraints)
- [Stardog Virtual Graphs](https://docs.stardog.com/virtual-graphs/)
- [TerminusDB GitHub](https://github.com/terminusdb/terminusdb)
- [Cayley GitHub](https://github.com/cayleygraph/cayley)

### Tooling
- [Buf Breaking Change Detection](https://buf.build/docs/breaking/overview)
- [`jsonschema` Rust Crate](https://crates.io/crates/jsonschema)
- [`rudof` (ShEx/SHACL Rust)](https://github.com/rudof-project/rudof)

### Axon Internal References
- [Axon Technical Requirements -- ESF Section](../01-frame/technical-requirements.md#5-schema-system)
- [FEAT-002 Schema Engine](../01-frame/features/FEAT-002-schema-engine.md)
- [FEAT-007 Entity-Graph Model](../01-frame/features/FEAT-007-entity-graph-model.md)

---

*This document is a living artifact. Updated as schema format decisions are made and ESF is designed.*
