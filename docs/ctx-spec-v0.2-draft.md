# CTX: A Portable Context Container Specification for AI Agents

Status: Draft
Version: 0.2

This document specifies the CTX artifact format and an optional registry-backed distribution profile for AI agent context. It is written as a working draft for review and editing. The core artifact specification is intended to be implementation-neutral. The distribution section is intentionally separated so the artifact format does not depend on any particular CLI or backend.

Normative keywords such as MUST, MUST NOT, SHOULD, and MAY are to be interpreted as described in RFC 2119 and RFC 8174.

## 1. Introduction

### 1.1 Purpose

CTX defines a portable, inspectable, and interoperable context artifact for AI agents. The goal is to make context a first-class artifact that can be created, inspected, updated, and shared without relying on an opaque memory backend or a tool-specific file convention.

A conformant implementation MUST be able to read and write CTX artifacts as defined in this document.

### 1.2 Problem statement

Current AI agent workflows typically rely on one of two extremes:

- opaque memory services that hide structure from the user, or
- informal file-based conventions that are easy to create but hard to standardize.

The first makes context hard to inspect and portability difficult. The second makes context easy to edit but difficult to exchange reliably between tools. CTX addresses this gap by defining a stable artifact structure and a separate distribution profile.

### 1.3 Scope

This specification defines:

- the CTX artifact model
- the artifact directory layout
- the manifest schema
- integrity and storage rules
- the notes layer
- the semantic and procedural index model
- lifecycle and drift handling
- an optional registry-backed distribution profile

This specification does not require:

- a particular CLI
- a particular cloud vendor
- a particular embedding model
- peer-to-peer distribution
- benchmark claims
- a specific storage engine for internal indexes

### 1.4 Design goals

A CTX implementation SHOULD optimize for:

- portability across tools and environments
- local-first ownership
- inspectability by humans and machines
- clear versioning and compatibility rules
- content integrity
- separation of semantic and procedural context
- optional raw-content storage
- explicit state transitions

## 2. Terminology

### 2.1 Artifact

A CTX artifact is the portable unit of context state. It is represented on disk as a directory named `<name>.ctx`.

### 2.2 Manifest

The manifest is the authoritative metadata document for a CTX artifact. It describes the artifact name, version, timestamps, configuration, source roots, file entries, and notes registry metadata.

### 2.3 Notes

The notes layer is the human-readable accumulation layer of the artifact. It is composed of markdown files intended to remain inspectable and editable.

### 2.4 Semantic index

A semantic index stores content-derived retrieval structures for meaning-oriented queries.

### 2.5 Procedural index

A procedural index stores structured records about workflows, outcomes, and execution history.

### 2.6 Blob

A blob is optional raw content stored by digest under the artifact. Blobs are content-addressed and immutable once written.

### 2.7 Drift

Drift is the condition where current source content differs from the hash recorded in the manifest.

### 2.8 Registry

A registry is a backend that stores, serves, or indexes CTX artifacts for remote access.

### 2.9 Publish

Publish is the act of writing the current local artifact state to a registry-backed remote instance.

### 2.10 Remote instance

A remote instance is a registry-hosted copy of a CTX artifact.

## 3. Design principles

### 3.1 Artifact-first design

The artifact, not the implementation, is the unit of portability. The artifact MUST remain valid independently of any particular CLI, API server, or MCP server.

### 3.2 Local-first ownership

A CTX artifact MUST remain usable without dependence on a hosted service. Remote publication is optional.

### 3.3 Human-readable accumulation

Notes files MUST remain readable as plain text markdown. The format SHOULD encourage human review and direct editing.

### 3.4 Content addressing

Stored content SHOULD be identified by digest. Hash-based identity provides integrity, deduplication, and stable references.

### 3.5 Dual retrieval model

Semantic and procedural knowledge SHOULD be represented as distinct retrieval surfaces.

### 3.6 Explicit state transitions

Changes SHOULD occur through explicit operations such as add, update, record, query, and publish. Silent mutation SHOULD be avoided where possible.

### 3.7 Optional raw content

Raw source content MUST be optional. A valid CTX artifact MAY contain only metadata, hashes, and derived context without storing source files in full.

## 4. Artifact model

### 4.1 Identity

A CTX artifact MUST be represented by a directory named `<name>.ctx`.

### 4.2 Required components

A conformant artifact MUST contain:

- `manifest.json`
- `index/`
- `notes/`

### 4.3 Optional components

A conformant artifact MAY contain:

- `blobs/`

### 4.4 Portability

An artifact MAY exist only locally, MAY be published to a registry, or MAY exist in both places.

### 4.5 Minimal valid artifact

The minimum valid CTX artifact MUST include a conforming manifest and the required directory structure, even if some internal directories are empty.

## 5. Directory layout

### 5.1 Required files

A conformant artifact MUST contain:

- `manifest.json`
- `notes/index.md`
- `notes/summary.md`

### 5.2 Required directories

A conformant artifact MUST contain:

- `index/`
- `notes/`

### 5.3 Optional directories

A conformant artifact MAY contain:

- `blobs/`

### 5.4 Internal layout

The internal layout of `index/` and `blobs/` MAY be implementation-defined, provided that the artifact remains conformant and the manifest accurately describes the artifact contents.

## 6. Manifest specification

### 6.1 Purpose

The manifest MUST serve as the authoritative index of artifact metadata and tracked content.

### 6.2 Top-level fields

The manifest MUST include:

- `version`
- `name`
- `created_at`
- `updated_at`
- `config`
- `sources`
- `notes`

### 6.3 Version

`version` MUST identify the specification version and MUST be used for compatibility checks.

### 6.4 Name

`name` MUST match the directory name without the `.ctx` suffix.

### 6.5 Timestamps

`created_at` and `updated_at` MUST be timestamps in ISO 8601 format.

### 6.6 Config

`config` SHOULD include fields such as:

- `store_raw_content`
- `notes_update_threshold_days`
- `notes_update_min_topics`
- `embedding_model`

Implementations MAY include additional runtime knobs, but fields not required by the spec SHOULD be treated as implementation details unless explicitly standardized.

### 6.7 Sources

`sources` MUST describe the indexed source roots and file entries associated with the artifact.

Each source root SHOULD record:

- the root path or identifier
- when it was added
- the files currently associated with that root

### 6.8 File entries

Each file entry MUST include:

- `path`
- `hash`
- `hash_at_index`
- `indexed_at`
- `type`

Each file entry MAY include:

- `blob_ref`

`hash_at_index` represents the digest observed when the file was indexed. If current source content later differs, the file is considered drifted.

### 6.9 Notes registry

The `notes` section of the manifest MUST track notes file entries and their hashes. Each entry SHOULD include:

- `path`
- `hash`
- `updated_at`

Implementations MAY record ownership metadata or editing provenance if available.

### 6.10 Unknown fields

Unknown fields SHOULD be ignored or preserved in a way that does not corrupt the artifact. Implementations MUST NOT treat unknown fields as a reason to silently invalidate otherwise conforming artifacts.

## 7. Storage and integrity model

### 7.1 Content addressing

All stored content SHOULD be identified using SHA-256 digests.

### 7.2 Digest format

Digests MUST use the format `sha256:<hex>`.

### 7.3 Blob immutability

Once written, a blob MUST NOT be modified.

### 7.4 Deduplication

Identical content MUST map to the same digest and SHOULD be stored once.

### 7.5 Default mode

By default, CTX SHOULD operate in hash-only mode and MUST NOT require raw content storage.

### 7.6 Raw-content mode

If raw content storage is enabled, content SHOULD be stored under `blobs/sha256/` or an equivalent implementation-defined location.

### 7.7 Drift detection

If current source content differs from `hash_at_index`, the file MUST be considered drifted.

Implementations SHOULD surface drift explicitly in status or inspection output.

## 8. Notes layer

### 8.1 Purpose

Notes MUST serve as the human-readable accumulation layer for agent context.

### 8.2 `notes/index.md`

`notes/index.md` MUST act as the hub file linking topic files and summarizing the artifact at a glance.

### 8.3 `notes/summary.md`

`notes/summary.md` MUST represent stable or promoted knowledge that should persist across sessions.

### 8.4 Topic files

Topic files MAY be added to capture domain-specific, project-specific, or session-specific context. Topic files SHOULD live under `notes/topics/` so that only `notes/summary.md` and `notes/index.md` remain at the notes root.

### 8.5 Editability

Humans SHOULD be allowed to edit notes files directly. The format SHOULD not require machine-only tooling for comprehension or maintenance.

### 8.6 Ownership

Notes registry entries SHOULD track the latest known editor or owner when such information is available.

### 8.7 Notes update

Implementations MAY define a notes update process that distills repeated stable information from topic files into a managed section of `summary.md`.

The notes update SHOULD preserve source history and SHOULD avoid destructive loss of useful provenance unless the implementation explicitly documents that behavior.

## 9. Index model

### 9.1 Semantic index

The semantic index SHOULD support meaning-oriented retrieval from source-derived content.

### 9.2 Procedural index

The procedural index SHOULD support retrieval of structured workflow records, outcomes, and repeated procedures.

### 9.3 Separation of intent

Semantic and procedural records MUST be logically distinct. Implementations SHOULD avoid conflating them in a single retrieval surface.

### 9.4 Implementation-defined internals

The on-disk layout and internal storage format of the indices MAY be implementation-defined.

### 9.5 Required record semantics

Implementations MUST preserve the logical meaning of procedural records and SHOULD preserve semantic provenance such as source path, chunk boundaries, or equivalent traceability fields.

## 10. Lifecycle and state management

### 10.1 Add

An implementation SHOULD provide a way to add source material to an artifact.

### 10.2 Update

An implementation SHOULD provide a way to re-index drifted content and refresh artifact state.

Implementation note (non-normative): operators benefit from a low-noise default where bulk
`add`/`update` runs emit a compact end-of-run summary and expose per-file skip reasons only in a
verbose mode.

### 10.3 Status

An implementation SHOULD provide a way to inspect drift and artifact health without mutating state.

An implementation MAY additionally provide a deeper health check (e.g. `ctx doctor`) that verifies blob integrity, notes registry consistency, and index presence. Such a command SHOULD run its checks concurrently, stream results fast-first so the operator receives immediate feedback, and offer a non-destructive repair mode that MAY include pruning orphan blobs, relocating stray notes topic files into `notes/topics/`, resyncing the notes registry, and rebuilding the index in place. Source files MUST NOT be deleted by a repair operation.

Implementation note (non-normative): when a deep health command is provided, a machine-readable output mode (for example JSON) improves automation and agent interoperability.

### 10.4 Query

An implementation SHOULD provide a way to retrieve semantic and procedural context.

### 10.5 Record

An implementation SHOULD provide a way to add structured procedural records.

### 10.6 Notes update cycle

If implemented, the notes update MUST preserve source history and SHOULD update `summary.md` rather than duplicating stabilized knowledge across many files.

### 10.7 Drift handling

Drift MUST be detected explicitly. Re-indexing SHOULD occur only when an explicit update or equivalent operation is invoked, unless a profile states otherwise.

## 11. Distribution profile

### 11.1 Purpose

This section defines how CTX artifacts MAY be published, fetched, discovered, and queried through a registry-backed remote instance.

This section is a profile layered on top of the artifact specification. It MUST NOT redefine the artifact format itself.

### 11.2 Registry abstraction

A registry MUST be treated as a backend abstraction. The spec MUST NOT require a single provider or cloud service.

A registry MAY be implemented using HTTP, object storage, or another documented addressable backend.

### 11.3 Publish semantics

Publish MUST write the current local artifact state to a remote instance.

Publish MAY create a new remote instance or overwrite an existing one, depending on registry policy.

### 11.4 Fetch semantics

Fetch MUST retrieve a remote instance for local use.

### 11.5 Remote query semantics

A remote instance SHOULD be queryable using the same logical artifact model as a local artifact.

### 11.6 Divergence

Local and remote instances MAY diverge over time.

### 11.7 Republish semantics

Republish SHOULD update the remote instance from the current local state.

### 11.8 Conflict resolution

If multiple replicas diverge, the default policy MAY be last-write-wins at the artifact level.

Fine-grained merge semantics SHOULD be considered out of scope unless explicitly defined in a later version.

### 11.9 Discovery

Artifact lookup and naming MAY be registry-defined or profile-defined, provided the mechanism is documented.

### 11.10 Non-goals

Peer-to-peer synchronization, federated merge, and backend standardization MUST NOT be required by the core specification.

## 12. Interoperability and conformance

### 12.1 Conformance language

MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY MUST be interpreted in the RFC 2119 and RFC 8174 sense.

### 12.2 Reader requirements

A compliant reader MUST be able to parse the artifact structure and preserve recognized data.

### 12.3 Writer requirements

A compliant writer MUST emit a valid manifest and conforming directory structure.

### 12.4 Version compatibility

New fields SHOULD be optional where possible.
A reader encountering unknown fields MUST NOT corrupt the artifact.

### 12.5 Minimum viable artifact

A conformant implementation SHOULD define the smallest valid artifact it can read and write. That minimum SHOULD still preserve the artifact/manifest/layout contract.

## 13. Security and privacy

### 13.1 Integrity verification

If raw content is present, implementations SHOULD verify digests when reading.

### 13.2 Sensitive notes content

Notes MAY contain sensitive information and SHOULD be protected accordingly.

### 13.3 Source path exposure

Distributed artifacts SHOULD minimize unnecessary leakage of local filesystem paths.

### 13.4 Registry trust model

Registry authentication and authorization SHOULD be implemented by the backend or profile.

### 13.5 Tamper handling

If integrity checks fail, an implementation MUST report failure and SHOULD refuse to trust the affected content.

## 14. Non-goals and future work

### 14.1 Peer-to-peer distribution

Peer-to-peer distribution is not required in v0.2.

### 14.2 Background sync

Background sync is not required in v0.2.

### 14.3 Merge automation

Merge automation is not required in v0.2.

### 14.4 Benchmarking

Benchmarking is not part of the normative specification.

### 14.5 Backend standardization

The specification does not standardize a single registry or cloud service.

### 14.6 CLI standardization

The CLI is a reference implementation concern, not a normative artifact requirement.

## Appendix A. Example artifact tree

```text
example.ctx/
  manifest.json
  index/
    semantic/
    procedural/
  notes/
    index.md
    summary.md
    topics/
      architecture.md
      decisions.md
  blobs/
    sha256/
      ...
```

## Appendix B. Example manifest sketch

```json
{
  "version": "0.2",
  "name": "example",
  "created_at": "2026-04-16T00:00:00Z",
  "updated_at": "2026-04-16T00:00:00Z",
  "config": {
    "store_raw_content": false,
    "notes_update_threshold_days": 7,
    "notes_update_min_topics": 3,
    "embedding_model": "openai:text-embedding-3-small"
  },
  "sources": [],
  "notes": {
    "files": [
      { "path": "notes/index.md", "hash": "sha256:...", "updated_at": "2026-04-16T00:00:00Z" },
      { "path": "notes/summary.md", "hash": "sha256:...", "updated_at": "2026-04-16T00:00:00Z" },
      { "path": "notes/topics/auth.md", "hash": "sha256:...", "updated_at": "2026-04-16T00:00:00Z" }
    ]
  }
}
```

## Appendix C. Notes for implementers

- This draft is intentionally compatible with the current CTX architecture while leaving room for future registry and sync work.
- The current reference implementation exposes a CLI, HTTP API, and MCP server, but the artifact spec MUST remain independent of those surfaces.
- The current codebase already models manifest metadata, source entries, notes registry entries, semantic and procedural indexes, and drift-sensitive updates.
- Publish and pull are treated here as a separate profile because they are not yet implemented in the reference CLI.
