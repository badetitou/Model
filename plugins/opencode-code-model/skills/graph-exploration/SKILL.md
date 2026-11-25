---
name: Graph exploration
description: Use the in-memory code model's graph tools (model_path, model_explain, model_communities, model_god_nodes, model_export_graph) to navigate relationships, find hotspots and export a data-only snapshot. Use for cross-cutting or architecture questions.
---

# graph-exploration

Use this when the question spans many entities: "how does A reach B?", "what is
this subsystem made of?", "where are the hotspots?". For a single symbol's
definition and immediate callers/callees, prefer `model-context`; for change
risk, prefer `model-impact`.

These tools come from the `model` MCP server. In Code Mode use
`tools.model.<tool>`; as direct tools they are named `model_<tool>`.

1. Call `status` if unsure a model is loaded. Resolve names to exact entity
   keys with `query` when needed.
2. Pick the graph question:
   - `path { from, to }` — shortest dependency chain between two keys.
     Optional `direction` (`callers`/`callees`/`both`, default `both`), `kinds`
     (empty = all relations), `depth`, `provenance`. Report each hop with its
     kind and `file:line`.
   - `explain { key }` — incoming/outgoing neighbours grouped by relation, with
     occurrence sites. Use `kinds` and `limit` to keep it short.
   - `god_nodes` — highest total-degree entities (in/out counts and
     `file:line`), optionally filtered by `kinds`; use `limit` (default 50).
   - `communities` — detected groups of related entities with sizes and
     cohesion; use `top` (default 50). Start here to orient yourself in an
     unfamiliar codebase.
3. For a reusable snapshot, call `export_graph { directory, top? }`. It creates
   the directory and writes `projection.json` (the `model.graph/1` data
   contract) and `REPORT.md` (summary, communities, ranked nodes and
   cross-community relations). It generates **no visualization**: no diagram,
   image or layout. Rendering, if desired, is the caller's job.

Always ground the answer in concrete keys and `file:line` from the results,
and say which filters (direction, kinds, depth, provenance) you used.
