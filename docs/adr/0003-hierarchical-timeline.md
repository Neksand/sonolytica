# ADR-0003: Hierarchical tracks plus typed relation edges

- **Status:** accepted
- **Date:** 2026-11-10

## Context

Independent models emit different objects: utterances, speaker turns, beats,
chords, silence regions, scenes. A flat list of events cannot express that a
speaker turn is part of an utterance, or that two turns overlap, without
re-deriving structure in every consumer. A full graph or RDF store can express
anything, but has no invariants and is awkward for agents to consume.

## Decision

The document holds:

- `tracks[]` grouping segments by modality;
- `segments[]` in one arena, each with `range`, optional `parent` (cross-track
  refinement) and a typed payload;
- `relations[]` with typed edges between segments (`overlaps`, `interrupts`,
  `contains`, `aligns_with`, `occurs_during`, `same_speaker_as`,
  `accompanies`).

Invariants are validated before a document is emitted: ids match positions,
references resolve, parents contain children and never share a track, ranges
stay inside the media, confidence is a finite probability.

## Consequences

- Agents can ask "what happens at t" (segment scan), "what refines this
  utterance" (parent), and "what overlaps it" (edges) without extra indexes.
- The schema stays extensible: a new modality adds a page of typed payload and,
  if needed, a relation kind — not a redesign.
- Cost: relations are directed and must be interpreted with their direction;
  this is documented per kind.
- Cost: JSON grows with word timings. Words are payloads of speech segments,
  not segments themselves, which keeps the arena small.

## Alternatives rejected

- Flat event list: loses hierarchy and invites ad-hoc glue in consumers.
- Free-form property graph: no invariants, heavy queries, poor ergonomics.
- Frame/tensor grid: large and semantically empty as an output format. Used
  internally by DSP only.
- Database-first: unnecessary for thousands of segments; a search index can be
  added without changing the document.
