# model-traits

Traits as data (name, typed props, relations carried, containment rules)
for Model, generated from the FM3 description of FamixJava. A trait is a bit
in a kind's `TraitMask` (`model-core`); `Model::of_trait` and
`EntityRef::has_trait` query it. `model-core` knows no trait names.

```rust
let mask = model_traits::mask(&["Named", "WithMembers"]);
model.of_trait(mask);                       // entities carrying both
model_traits::mask_of_fm3_class("Famix-Java-Entities.Class"); // for define_kind
```

- `traits.json` (hand-written): our traits, the FM3 traits each aggregates,
  and the `model-core` relation kinds it carries.
- `src/generated.rs` (checked in): props (stored, non-derived, builtin typed),
  relations, containment rules (multivalued property whose opposite is a
  container) and the per-class trait table.

## Regenerate

```bash
cargo run -p model-traits --example generate [fm3.json]
```

Default input: the FamixJava subset in `model-fm3`'s test fixtures; pass a
full `FMJSONPrinter` export of FamixJava to use the real thing. The test
`generated_data_is_up_to_date` fails when `generated.rs` is stale.
