# Model — notes for Claude

Language-neutral code model from tree-sitter + SCIP, with language
auto-detection. Design and roadmap: `docs/design.md`. Extracted from
Structure (`../Structure`), its first client.

## Commands

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

## Map

- `crates/model-core` — metamodel (`Entity`, `EntityKind`, `Relation`,
  `RelationKind`, `Provenance`, `Props`), `ModelBuilder` (get-or-create by
  key), `Model` (navigation; JSON `{entities, relations}`, indexes rebuilt on
  load). serde only.
- `crates/model-detect` — `detect(root)`: languages, file counts, project
  roots; `languages.rs` is the data table (`LanguageSpec`).

## Rules

1. Before calling a change done: fmt, clippy `-D warnings` and tests pass.
2. No `unwrap()`/`expect()` outside tests and `main`.
3. No `unsafe` without a `// SAFETY:` comment and the user's agreement.
4. New dependencies only when justified; grammars behind Cargo features.
5. Language-specific knowledge goes in data (language table, query files) or
   enrichers, never in `model-core`.
6. Commit only when asked.
