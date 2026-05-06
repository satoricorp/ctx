# CTX: A Portable Context Container Specification for AI Agents

Draft white paper for review

## Abstract

AI agents increasingly need persistent context that survives across sessions, tools, and environments. Existing approaches either hide state inside opaque memory services or rely on informal files that are easy to create but hard to standardize. CTX addresses this gap with a portable context container specification. A CTX artifact is a directory-based container composed of a manifest, structured retrieval indices, a human-readable notes layer, and optional raw-content blobs. The design keeps context locally inspectable while making it possible to publish or query a remote instance through a registry-backed distribution profile. We argue that portability is valuable not because context is merely stored somewhere else, but because the artifact itself remains legible, versioned, and interoperable across implementations. In this paper, efficacy is supporting evidence: CTX works because the structure makes persistent context easier to inspect, update, and reuse.

## 1. Introduction (main text)

AI agents are increasingly expected to preserve work over time. A useful agent remembers the shape of a project, the decisions that led to it, and the procedures that were successful in the past. Yet most current systems treat this context as an internal implementation detail, or they expose it through ad hoc files and tool-specific conventions.

That creates a recurring problem. When context is hidden inside a memory service, users cannot easily inspect it, edit it, or move it. When context is stored in plain files without a shared structure, it becomes flexible but fragmented: one tool’s notes are another tool’s unreadable pile of text.

CTX proposes a middle path. It treats context as a first-class portable artifact. The artifact has a defined shape, explicit metadata, content-addressed integrity, and a human-readable accumulation layer. The result is not just a place to store context, but a standard way to package it.

## 2. Contributions (main text)

This paper makes four primary contributions.

First, it defines CTX as an artifact-centric model for portable context rather than as an internal memory abstraction.

Second, it specifies the major components of the artifact: manifest, indices, notes, and optional blobs.

Third, it separates the core artifact format from the distribution profile so remote publication and remote query can evolve independently of the on-disk structure.

Fourth, it grounds the proposal in a reference implementation that already exposes context through local and remote interfaces, while keeping the specification itself implementation-neutral.

## 3. Background and related work (main text)

CTX sits between two familiar patterns. On one side are opaque memory services that may be operationally convenient but are difficult to inspect and hard to port. On the other are informal note folders and markdown workflows that are transparent but not standardized.

The design also borrows a general lesson from widely adopted technical specifications: useful standards separate format from transport, keep the core artifact readable, and define versioning and integrity behavior explicitly. The goal here is not to mirror any one ecosystem, but to apply that structural discipline to AI context.

## 4. Why a context artifact matters (main text)

The central claim of CTX is that portability requires structure. A context system is only truly portable if another implementation can understand what a context object is, what parts are authoritative, what parts are optional, and how to reason about integrity and drift.

CTX therefore distinguishes among several kinds of state:

- the manifest, which is the authoritative index
- the semantic index, which supports meaning-oriented retrieval
- the procedural index, which stores workflow and outcome records
- the notes layer, which is meant to remain human-readable and editable
- optional blobs, which preserve raw content when desired

That separation is the point. It allows context to be both machine-useful and human-legible.

## 5. CTX artifact model (main text)

A CTX artifact is a directory named `<name>.ctx`. The artifact is the unit of portability. It can live only on a local machine, or it can be published to a remote registry-backed instance. The artifact format itself does not require a particular CLI, server, or cloud vendor.

At minimum, a CTX artifact contains:

- `manifest.json`
- `index/`
- `notes/`

It may also contain `blobs/` if raw content storage is enabled.

The manifest binds the artifact together. It records the artifact name, version, timestamps, configuration, source roots, file entries, and notes file metadata. The notes layer provides a compact, inspectable form of long-lived context. The indices capture the structured retrieval surface used by agents and tools.

### 5.1 Manifest and integrity

The manifest is the artifact’s source of truth. It records the version, timestamps, configuration, and the material associated with the artifact. CTX’s current implementation already models source entries, file hashes, hash-at-index values, and notes registry entries.

The manifest also supports drift detection. If a file changes after it has been indexed, the artifact should know that the indexed representation may no longer match the source. That explicit drift signal is important because it keeps state transitions visible. Instead of silently rewriting context, CTX can report what changed and let the user or tool decide what to do next.

Integrity is based on content addressing. Hashes allow the artifact to detect tampering, deduplicate identical content, and preserve stable references over time. Raw-content storage is optional. CTX can therefore work in a hash-only mode when the user wants a lighter artifact, or in a richer mode when the original content should be retained inside the container.

### 5.2 Notes layer

A key design choice in CTX is to keep a human-readable accumulation layer at the center of the system.

The notes layer is not an implementation detail. It is the durable, editable surface where useful context accumulates over time. In the current architecture, `notes/index.md` acts as a hub or table of contents, `notes/summary.md` stores stabilized knowledge, and topic files capture focused notes for specific concerns.

This matters because persistent context is not only for model consumption. It is also for humans who need to inspect what an agent knows, correct it, or refine it. A good context system should therefore remain understandable without specialized tooling.

### 5.3 Retrieval indices

A CTX artifact separates semantic and procedural retrieval on purpose.

The semantic side is for meaning-oriented search over source-derived content. The procedural side stores structured records about tasks, steps, outcomes, and failure modes. The point of the split is not merely implementation convenience. It is to preserve two different kinds of context that AI agents use in practice.

### 5.4 Optional raw-content blobs

Blobs are optional. That is an important part of the design.

A CTX artifact does not need to store all raw source content in order to be useful. In many cases, hashes, summaries, extracted records, and notes entries are enough. When raw content is needed, it can be stored under the artifact as a content-addressed blob.

## 6. Distribution profile (main text)

Distribution is separated from the artifact format itself.

A CTX artifact may be published to a registry-backed remote instance. In that model, local and remote copies are instances of the same artifact state rather than entirely different formats. The registry is an abstraction: it could be implemented by an HTTP API, object storage, or another addressable backend.

The important semantics are simple. Publish writes the current local state to a remote instance. Fetch retrieves the remote instance. Query may target the local artifact or the remote instance. If the local artifact drifts from the remote one, republishing updates the remote state. For the first version of this idea, the simplest policy is enough: last write wins at the artifact level, and more complex merging is future work.

This model is intentionally modest. It avoids peer-to-peer synchronization and complicated conflict resolution. The goal is to make remote publication and remote query possible without making distribution the centerpiece of the standard.

## 7. Why this should work (main text)

CTX is not trying to prove portability with a benchmark first. Instead, it argues for portability by design.

The system should work because it makes the hard parts explicit:

- what is authoritative
- what is derived
- what is human-editable
- what is hash-verified
- what is local-only versus remotely published
- how drift is detected
- how context is accumulated over time

This makes CTX easier to inspect and easier to trust. It also makes it easier for multiple tools to share one context model without agreeing on a single backend implementation.

Supportive evidence for the design comes from the architecture itself: the repository already separates semantic and procedural retrieval, exposes context through a CLI, HTTP API, and MCP server, and stores artifact metadata in a manifest that tracks source and notes state. In other words, CTX already behaves like a portable artifact system; the specification simply makes that shape explicit.

## 8. Limitations and future work (main text)

Several interesting problems are intentionally out of scope for this draft.

First, peer-to-peer distribution is not part of the current design. Second, advanced conflict resolution is not defined beyond a simple artifact-level overwrite model for registry publication. Third, backend standardization is avoided so that the format can outlive any one storage provider. Fourth, benchmark-heavy evaluation is left for later work.

Those omissions are deliberate. The purpose of this paper is to define the artifact and the core distribution profile clearly enough that implementation and experimentation can proceed without ambiguity.

## 9. Conclusion (main text)

CTX proposes that context for AI agents should be treated as a portable artifact rather than a hidden service. By combining a manifest, structured indices, a human-readable notes layer, optional blobs, and a separate distribution profile, CTX provides a concrete format for persistent context that is inspectable, local-first, and interoperable.

The main contribution is the specification itself. If the artifact can be standardized, then the ecosystem around it can evolve independently. That is the practical promise of CTX: portable context, not portable lock-in.

## Appendix A. Example artifact tree (appendix)

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

## Appendix B. Example manifest sketch (appendix)

```json
{
  "version": "0.2",
  "name": "example",
  "created_at": "2026-04-16T00:00:00Z",
  "updated_at": "2026-04-16T00:00:00Z",
  "config": {
    "store_raw_content": false,
    "promotion_threshold_days": 7,
    "promotion_min_occurrences": 3,
    "embedding_model": "openai:text-embedding-3-small"
  },
  "sources": [],
  "notes": { "files": [] }
}
```

## Appendix C. Implementation notes (appendix)

- This draft is intentionally compatible with the current CTX architecture while leaving room for future registry and sync work.
- The current reference implementation exposes a CLI, HTTP API, and MCP server, but the artifact spec MUST remain independent of those surfaces.
- The current codebase already models manifest metadata, source entries, notes registry entries, semantic and procedural indexes, and drift-sensitive updates.
- Publish and pull are treated here as a separate profile because they are not yet implemented in the reference CLI.
