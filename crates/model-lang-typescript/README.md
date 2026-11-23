# model-lang-typescript

TypeScript/TSX **enricher**: it runs on a `ModelBuilder` that already holds the
TypeScript entities (`model-treesitter`, with or without SCIP linking) and adds
HTTP facts. Ported from Structure's `lang-ts/src/http.rs`; Structure's IR types
are not ported, everything is expressed with the builder API.

```rust
let mut warnings = Vec::new();
let stats = model_lang_typescript::enrich(&mut builder, root, &mut warnings);
// or, from memory:
model_lang_typescript::enrich_sources(&mut builder, &[("src/a.ts", text)], &mut warnings);
```

`model_factory::build` calls `enrich` after linking when TypeScript is
detected. Both entry points are idempotent and deterministic.

## What it adds

| Fact | Model |
|---|---|
| a request made by the code | entity of kind `http.call`, child of the innermost entity containing it (method, arrow function, else the file); name `GET /users/{id}`; props `http.verb`, `http.path`, `http.library` (`fetch`, `axios`, `angular`) |
| a route served by the code | entity of kind `http.endpoint`, same placement; props `http.verb`, `http.path`, `http.framework` (`nestjs`, `express`) |
| link owner → fact | relation `Other("http.calls")` / `Other("http.endpoints")`, `Provenance::Frontend`, site = the call / the decorator |
| what a join needs | list props `http.calls` / `http.endpoints` on the owner (`["GET /users/{id}"]`) |

Kinds are data (`kinds.json`: archetype `Variable`, trait `Named`, GraphQL
concepts `HttpCall`/`httpCalls`, `HttpEndpoint`/`httpEndpoints`), loaded with
`define_kind`; `declare_kinds` defines them without a model. Keys hang under the
owner: `…/UserService#list().call-L7C18:` (`Meta` descriptor, line and column of
the site). Cross-language linking (TS call ⨝ Java endpoint) is *not* done here:
it is a `Derived` analysis over these props (`docs/design.md` §7).

Paths are normalised like Structure's: leading `/`, no query, no
`scheme://host`, no trailing `/`; `${x}` and `+ x` become `{x}`; Express/Nest
`:id` becomes `{id}`; an unreadable path is `?`, an unreadable verb `ANY`.

### Recognised

- Calls: `fetch(url, init?)`; `axios(...)`, `axios.get|post|put|delete|patch(...)`;
  Angular `HttpClient` methods (`get`, `post`..., `request(verb, url)`) on
  names typed `HttpClient` or initialised with `inject(HttpClient)`, and the
  conventional `http`.
- Endpoints: NestJS `@Controller('p')` / `@Controller({ path })` with
  `@Get|Post|Put|Delete|Patch|Head|Options|All('x')` on methods (decorators
  before or inside the member, before or after `export`);
  Express-style `app.get('/x', handler)` (also `post`, `put`, `delete`,
  `patch`, `head`, `options`, `all`) on a receiver initialised with `express()`,
  `Router()`, `fastify()`, `new Hono()`... or named `app`, `router`, `server`,
  `api`, `*Router`; the path must be a literal starting with `/` and a handler
  must follow.

### Not done

- `app.use('/prefix', router)` mounting (Express endpoint paths are relative to
  their router), `app.route('/x').get(...)` chains, NestJS `@Version` and
  global prefixes, Fastify `route({ method, url })`, Angular router routes
  (client-side, not HTTP endpoints), `HttpClient` wrapped in another service.
- Request bodies, headers, response types.
- A URL held in a variable is resolved one level (a `const` in the same
  function), as in Structure.

## Famix compatibility (FamixTypeScript)

Target metamodel: [fuhrmanator/FamixTypeScript](https://github.com/fuhrmanator/FamixTypeScript),
package `Famix-TypeScript-Entities`, checked at commit `de43767` (generator
`FamixTypeScriptGenerator`, the `*.class.st` files). The repository does **not**
publish an FM3 JSON export and carries no licence file, so no export is vendored:
`tests/fixtures/famixtypescript-classes.txt` lists the class names (facts) and
a test checks that every `Famix-TypeScript-Entities.*` name carried by a kind
(here and in `model-treesitter/kinds.json`) is in that list.

Class names below are **confirmed** unless marked. `fm3` = what `KindInfo.fm3`
holds today; "archetype" = the kind is a plain archetype default (`class`,
`method`...), which has no `fm3` field: the Moose export derives the class from
the archetype (`model-moose`'s `mapping`).

| Model kind (`typescript.scm`) | Archetype | `fm3` set | FamixTypeScript class | Note |
|---|---|---|---|---|
| `ts.module` (file entity) | File | `Module` | `Module` | the script; Famix `File`/`Folder` come from anchors |
| directory namespaces | Namespace | none | `Folder` (exists; use unconfirmed) | Model makes directories namespaces |
| `class` | Class | none (archetype) | `Class` | `ParametricClass` for generics: not distinguished |
| `ts.abstract_class` | Class | `Class` | `Class` | abstractness is `TCanBeAbstract`, not a class |
| `ts.interface` | Interface | `Interface` | `Interface` | `ParametricInterface` for generics: not distinguished |
| `enum` | Enum | none (archetype) | `Enum` | members (`EnumValue`) not captured by the query |
| `ts.type_alias` | TypeAlias | `Alias` | `Alias` | the metamodel has no `TypeAlias`; `Alias` (a named, importable entity) is the only candidate, its use for `type X = ...` is **unconfirmed** (was `TypeAlias`, which does not exist) |
| `function` | Function | none (archetype) | `Function` | `ParametricFunction` for generics |
| `ts.arrow_function` | Function | `ArrowFunction` | `ArrowFunction` | `ParametricArrowFunction` for generics |
| `ts.namespace` | Namespace | `Namespace` | `Namespace` | `Module` also has `isNamespace`: which one an exporter picks for `namespace X {}` is unknown |
| `method`, `method_signature`, `abstract_method_signature` | Method | none (archetype) | `Method` | |
| `constructor` | Constructor | none (archetype) | `Method` (`THasKind` kind `constructor`) | no `Constructor` class |
| getters/setters | Method | none | `Accessor` (subclass of `Method`) | not distinguished by the query |
| `field` (class member), `property` (interface member) | Field / Property | none (archetype) | `Property` | |
| `parameter` | Variable | none (shared with Java) | `Parameter` | |
| `local_variable` | Variable | none (shared) | `Variable` | |
| `type_parameter` | Variable | none (shared) | `TypeParameter` | |
| `comment` | Variable | none (shared) | `Comment` (`JSDoc` for doc comments) | |
| decorators | prop `decorators` | none | `Decorator` | the TS query records names as a prop, no entity; the shared `annotation` kind is Java's |
| `http.call`, `http.endpoint` | Variable | none | none | not Famix concepts: no class invented |

Relations: `inherits` ↔ `Inheritance`, `implements` ↔ `Implementation`,
`calls` ↔ `Invocation`, `accesses` ↔ `Access`, `references` ↔ `Reference`,
`imports` ↔ `ImportClause`, `declared_type` ↔ `EntityTyping` (all exist in the
package; the Moose export maps by relation, not here).

Why shared kinds (`parameter`, `local_variable`, `type_parameter`, `comment`,
`annotation`) have no `fm3`: a kind names one class and these are shared with
Java (`Famix-Java-Entities.*`). A language-qualified mapping, or per-language
kinds, is needed before they can carry one (open question).
