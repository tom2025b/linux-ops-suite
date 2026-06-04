# Contract Rules

These rules govern every cross-tool file contract in the suite. The contract files
in [`../contracts/`](../contracts/) are the **source of truth**; tools conform to them.

## Required of every export

1. **`schema_version`** — required, an **integer** major version. Present on every
   contract. Bumped only on a breaking change (see below).
2. **A timestamp** — every export should carry when it was generated. The preferred
   field name is **`generated_at`** (RFC3339 or `YYYY-MM-DD`).
3. **`source_tool`** — recommended; the producing tool's short name (e.g. `"bulwark"`).

> Optional-but-recommended means a consumer must not crash if it is absent.

## Versioning

- **Additive changes** (new optional fields) keep the **same** `schema_version`.
- **Renames or removals** of existing fields require a **new major** `schema_version`.
- Consumers **must ignore unknown fields** (schemas use `additionalProperties: true`).
- Producers **must keep field names stable** within a major version.

## Resilience

- A **missing or unreadable** optional producer feed must **never crash RexOps**.
  RexOps degrades gracefully and reports the producer as unavailable.
- Consumers validate `schema_version` first and refuse only on a major they don't know.

## Known accepted deviation (v1)

ToolFoundry's shipped `rexops-feed` (`fixtures/rexops_feed_v1.json`, with a passing
contract test) predates these rules. It uses:

- **`as_of`** instead of `generated_at`, and
- **no `source_tool`** field.

For **v1 this is accepted as-is** — we conform the rules to the shipped reality rather
than force a tool change. The
[`toolfoundry.rexops-feed`](../contracts/toolfoundry.rexops-feed.schema.json) schema
documents the real shape. Future contracts should prefer `generated_at` + `source_tool`;
ToolFoundry may align at its next major version, not before.
