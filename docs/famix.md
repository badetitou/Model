# Model and Famix

Status: analysis, written for epic [#1](https://github.com/badetitou/Model/issues/1)
(Famix-like models, on demand, exportable to Moose). No code yet.

Model's metamodel is small on purpose (design §3: 15 closed entity kinds,
untyped `props`, five relation kinds). [Famix](https://github.com/moosetechnology/Famix)
is much richer: FamixJava alone has about 125 traits, 74 classes and 311
properties, of which 196 are derived. This document

1. summarises Fame/FM3, the meta-meta-model Famix is written in (§1);
2. compares FamixJava with Model, trait by trait (§2) and lists the gaps (§3);
3. summarises the plan of the epic (§4).

Sources: the trait definitions of `Famix-Traits`, the Java generator
(`FamixJavaGenerator`), the Fame documentation of Moose, and the FM3 listing
of the FamixJava metamodel.

## 1. Fame / FM3 in short

**Fame** is the meta-meta-model; **FM3** is its serialised form (a metamodel is
itself a model whose entities are FM3 entities).

```
Package ─┬─ Class  ── superclass (single), traits (many), properties
         └─ Trait  ── traits (many), properties
Property ── name, type, multivalued, container, derived, opposite
```

- **Package** groups classes and traits (`Famix-Java-Entities`, `Famix-Traits`).
- **Class** has at most one superclass, any number of traits, and owns
  properties. **Trait** is a reusable bundle of properties and behaviour; a
  trait may use other traits. A class *composes* traits: `FamixJavaMethod` is
  a `ContainerEntity` plus `TMethod`, `TCanBeFinal`, `TCanBeAbstract`,
  `THasVisibility`, `TWithExceptions`, `TWithComments`...
- **Property** belongs to a class or trait and has:
  - `type`: a class/trait, or a primitive (`Boolean`, `Number`, `String`,
    `Object`);
  - `multivalued`: a collection rather than one value;
  - `container`: the owner side of a composition (`parentType`,
    `parentPackage`): it designates the entity that contains this one;
  - `derived`: computed from other properties, never stored (`methods` of a
    type is the opposite of `parentType` and is derived);
  - `opposite`: the mirror property on the other class. Setting one end sets
    the other. Relations are therefore pairs of properties, and
    associations (`Invocation`, `Access`, `Inheritance`...) are reified as
    entities with two such pairs (`sender`/`outgoingInvocations`,
    `candidates`/`incomingInvocations`).

Containment is a property flagged `container`, exactly like Model's `parent`.

### JSON model format

A model (or metamodel) is a JSON array of entities:

```json
[
  { "FM3": "Famix-Java-Entities.Class", "id": 1, "name": "A",
    "parentPackage": { "ref": 3 } },
  { "FM3": "Famix-Java-Entities.Method", "id": 2, "name": "run",
    "parentType": { "ref": 1 } },
  { "FM3": "Famix-Java-Entities.Package", "id": 3, "name": "com.acme" }
]
```

- `FM3` is the first key: the qualified type name, `<package>.<class>`.
- `id` must come right after `FM3`; it is optional, and only needed if the
  entity is referred to.
- A property whose value is another entity is either the entity nested
  inline, or a reference `{ "ref": n }`. Collections are arrays of these.
  Forward references are allowed (importers accept dangling references until
  the end of the file).
- Primitive values are JSON strings, numbers and booleans. Derived properties
  are not exported, except `numberOfLinesOfCode` and `cyclomaticComplexity`
  computed by `model-analysis` at export time.
- Because properties are opposites, the file lists only one end of each pair
  (usually the container or the owning side); the importer rebuilds the other.
- A metamodel is exported in the same way, with `FM3` equal to `FM3.Package`,
  `FM3.Class`, `FM3.Trait`, `FM3.Property`. Property types appear as
  `{ "ref": n }`, or by name for the built-in primitives. This is what the
  planned `model-fm3` crate reads (#3): the FamixJava metamodel is data, not
  code.

## 2. Conformity matrix: FamixJava vs Model

Legend:

- **covered**: represented today, possibly with a coarser name;
- **derivable**: not stored, but computable from what is stored (an analysis,
  or a query to add);
- **gap**: needs new extraction (a tree-sitter query, an SCIP fact) or a new
  relation kind: see §3;
- **out of scope**: metrics, file-system, tool-specific. Not a goal.

Model entities are `EntityKind`s plus `props`. "Prov." says where the data
would come from: `TS` (tree-sitter), `SCIP`, or `calc` (derived from the
model).

### 2.1 Entities (FamixJava classes)

| FamixJava class | Model | Status | Notes |
|---|---|---|---|
| `Package` | `Namespace` | covered | Java `package` declarations; one entity per package |
| `Class` | `Class` | covered | |
| `Interface` | `Interface` | covered | |
| `Enum` | `Enum` | covered | |
| `EnumValue` | `Constant` / `Field` | covered | not distinguished from other fields today |
| `AnnotationType` | `Interface` (@interface) | gap | the declaration is a type, but not flagged as an annotation type |
| `AnnotationTypeAttribute` | `Method` | gap | members of `@interface`; same flag issue |
| `Exception` | `Class` | derivable | a `Class` that inherits from `Throwable` (needs resolution of `Inherits`) |
| `Method` | `Method` / `Constructor` | covered | `isConstructor` derives from the kind; `Initializer` (static block) has no kind |
| `Attribute` | `Field` | covered | |
| `Parameter` | none | gap | no entity per parameter (TS query on `formal_parameter`; SCIP has none either) |
| `LocalVariable` | none | gap | no entity for locals (`local_variable_declaration`) |
| `ImplicitVariable` (`this`, `super`) | none | gap | only needed as the receiver of invocations |
| `UnknownVariable` | none | out of scope | a placeholder for unresolved names |
| `TypeParameter` (`<T>`) | none | gap | |
| `Wildcard` | none | gap | |
| `ParameterizedType`-style entities (`ParametricClass`, `ParametricInterface`, `ParametricMethod`) | none | gap | a class or method with type parameters; `ParametricClass` is a flag on `Class` plus `TypeParameter` children |
| `PrimitiveType` | `Class` with `external` | derivable | `int`, `void`... could be synthesised as externals |
| `AnnotationInstance`, `AnnotationInstanceAttribute` | `props["java.annotations"]` (names only) | gap | the annotation's type, its target and its attribute values are lost |
| `Comment` | none | gap | |
| `Lambda` | none | gap | |
| `Import` | `Imports` relation | covered | file to imported entity; Famix's import is an association from a type, not from the file |
| `Inheritance` | `Inherits` relation | covered | class to superclass. No separate entity |
| `Implementation` | `Implements` relation | covered | |
| `Invocation` | `Calls` relation | covered | site-level; the receiver and signature are not kept (§3.2) |
| `Access` | `References` relation | gap | `References` is not split read/write and fields are not always targets (§3.2) |
| `Reference` | `References` relation | covered | type use (`new X()`, a declared type) |
| `EntityTyping` (declared type) | none | gap | variable/method to its type: `declared_type` (§3.2) |
| `Concretization`, `Parametric*` associations | none | gap | type arguments of a use: `List<String>` |
| `Namespace`, `Module` (non-Java) | `Namespace` | covered | |
| `FileAnchor`, `IndexedFileAnchor` | `file` + `range` | covered | `Range` is line/column; Famix wants character offsets (`startPos`, `endPos`): derivable from the file |
| `Folder`, `File` | `File` entity | covered | `Folder` is a `Namespace` under path-language models; metrics and encoding are out of scope |
| `SourceLanguage` | `language` | covered | |
| `*Group` (`FamixClassGroup`...) | none | out of scope | Moose UI helper |

### 2.2 Properties, by trait

Traits come from `Famix-Traits`; the "composed by" column lists the Java
classes that use the trait, from `FamixJavaGenerator`.

| Trait | Properties | Composed by | Model | Status |
|---|---|---|---|---|
| `TNamedEntity` | `name` | everything named | `name` | covered |
| `TSourceEntity` | `sourceAnchor`, `numberOfLinesOfCode` | everything with source | `file`, `range`; LOC = lines of `range` (`model_analysis::lines_of_code`, also exported to Moose) | covered / derivable |
| `THasSignature` | `signature` | Method, Invocation, Lambda | the SCIP-like key includes parameter types | derivable |
| `THasVisibility` | `visibility` | Class, Interface, Enum, Method, Attribute, AnnotationType | `props["visibility"]` is defined, but the Java query does not set it | gap (TS modifier query) |
| `TCanBeAbstract` | `isAbstract` | Class, Method | none | gap (TS modifier query) |
| `TCanBeFinal` | `isFinal` | Class, Interface, Method, Attribute, Parameter, LocalVariable | none | gap (TS modifier query) |
| `TCanBeClassSide` | `isClassSide` (static) | Class, Interface, Method, Attribute | none | gap (TS modifier query) |
| `TCanBeSynchronized`, `TCanBeVolatile`, `TCanBeTransient` (Java) | `isSynchronized`, `isVolatile`, `isTransient` | Method, Attribute | none | gap (TS modifier query) |
| `THasKind` | `kind` (`constructor`, `getter`...) | Method | `EntityKind::Constructor` | covered / derivable |
| `TCanBeStub` | `isStub` | Type, Package, StructuralEntity | `external` | covered |
| `TPackageable` / `TPackage` | `parentPackage`, `childEntities` | Class, Interface, Enum, AnnotationType, Package | `parent` (Namespace ⊃ Type) | covered |
| `TWithClasses`, `TWithTypes`, `TWithAnnotationTypes` | `types`, `definedAnnotationTypes` | Package, ContainerEntity | children by `parent` | covered |
| `TWithMethods` / `TMethod` | `methods`, `parentType` | Type | `parent` | covered |
| `TWithAttributes` / `TAttribute` | `attributes`, `parentType` | Class, Interface, Enum, AnnotationType | `parent` of a `Field` | covered |
| `TWithParameters` / `TParameter` | `parameters`, `parentBehaviouralEntity` | Method, Lambda | none | gap |
| `TWithLocalVariables` / `TLocalVariable` | `localVariables`, `parentBehaviouralEntity` | Method, Lambda | none | gap |
| `TWithImplicitVariables` / `TImplicitVariable` | `implicitVariables` | Method | none | gap |
| `TWithLambdas` / `TLambda` | `lambdas`, `lambdaContainer` | Method, Lambda | none | gap |
| `TWithStatements` | `statements` | Method, Lambda | none | out of scope (AST level) |
| `TTypedEntity` / `TEntityTyping` | `typing`, `declaredType` | Method, Variable | none | gap (`declared_type` relation) |
| `TWithInheritances` / `TInheritance` | `superInheritances`, `subInheritances`, `superclass`, `subclass` | Class, Interface, Enum, AnnotationType | `Inherits` relation | covered |
| `TCanImplement` / `TImplementable` / `TImplementation` | `interfaceImplementations`, `implementations`, `interface`, `implementingClass` | Class, Interface | `Implements` relation | covered |
| `TWithInvocations` / `TInvocable` / `TInvocation` | `outgoingInvocations`, `incomingInvocations`, `candidates`, `sender`, `receiver` | Method, Lambda | `Calls` relation, sites | covered (receiver gap) |
| `TInvocationsReceiver` | `receivingInvocations` | Class, Interface, Enum, Variable | none | gap |
| `TWithAccesses` / `TAccessible` / `TAccess` | `accesses`, `incomingAccesses`, `accessor`, `candidates`, `isWrite` | Method, Variable | `References` | gap (read/write, target kind) |
| `TWithReferences` / `TReferenceable` / `TReference` | `outgoingReferences`, `incomingReferences`, `referencer`, `referredEntity` | Class, Method... | `References` relation | covered |
| `TWithImports` / `TImportable` / `TImport` | `imports`, `incomingImports`, `importedEntity`, `importingEntity` | Class, Interface, Enum | `Imports` relation, from file | covered (source side differs) |
| `TWithExceptions` / `TThrowable` / `TException` | `declaredExceptions`, `thrownExceptions`, `caughtExceptions` | Method, Exception, TypeParameter | none | gap (`throws`, plus `throw` and `catch` sites) |
| `TWithAnnotationInstances` / `TAnnotationInstance` | `annotationInstances`, `annotatedEntity`, `annotationType`, `attributes` | every NamedEntity | `props["java.annotations"]` | gap (annotation type and attribute values) |
| `TAnnotationType` / `TAnnotationTypeAttribute` / `TAnnotationInstanceAttribute` | `instances`, `value`, `annotationTypeAttribute` | AnnotationType, AnnotationInstance | none | gap |
| `TWithComments` / `TComment` | `comments`, `commentedEntity` | Class, Method, Attribute, Package... | none | gap |
| `TParametricEntity` / `TTypeParameter` | `typeParameters`, `genericEntity`, `concretizations` | Class, Interface, Method | none | gap |
| `TTypeArgument` / `TConcretization` | `outgoingConcretizations`, `typeArgument` | Class, Interface, Enum, PrimitiveType, TypeParameter, Wildcard | none | gap |
| `TBounded` / `TBound` (Java) | `lowerBound`, `upperBound` | TypeParameter, Wildcard | none | gap |
| `TEnum` / `TEnumValue` / `TWithEnumValues` | `enumValues`, `parentEnum` | Enum | `Enum` ⊃ `Constant` | covered / derivable |
| `TPrimitiveType` | | PrimitiveType | external class by name | derivable |
| `TClass` | `isTestCase` (derived: JUnit) | Class | none | derivable (annotation `@Test` / supertype) |
| `TSourceLanguage` | `sourceLanguage` | model | `language` | covered |
| `TFileAnchor`, `TIndexedFileNavigation` | `fileName`, `startPos`, `endPos`, `encoding` | anchors | `file`, `range` | covered / derivable |
| `TClassMetrics`, `TMethodMetrics`, `TLCOMMetrics`, `TCohesionCouplingMetrics` | `numberOfMethods`, `cyclomaticComplexity`, ... | Class, Method | `CodeMetrics` + `@complexity.branch` facts | LOC and cyclomatic complexity derived and exported; the rest out of scope |
| `TFile`, `TFolder`, `TFileSystemEntity`, `TFileNavigation` | | | `File` entity | out of scope |
| `TTemplate`, `TTemplateUser`, `TFileInclude`, `TDereferencedInvocation`, `TGlobalVariable`, `TFunction`, `TModule`, `TTraitUsage`, `TTrait`, `TTypeAlias` | | other languages (C, C++, TypeScript, Pharo) | `Function`, `Variable`, `Trait`, `TypeAlias` kinds | covered where the kind exists; the rest is for generalisation (#13) |

### 2.3 Reading the matrix

- Structure (containment, types, members) is covered. Model keeps one
  relation for the two sides of a Famix pair; Famix reifies each association
  as an entity (`Invocation`, `Inheritance`...). Model's relation plus its
  sites carries the same information, without an entity per association.
- What is *derivable* in Famix (`methods`, `subInheritances`,
  `incomingInvocations`, `isOverriding`, ...) is derivable in Model by
  following the relation backwards.
- What is missing is mostly **fine-grained entities inside a method**
  (parameters, locals, lambdas, type parameters) and **typed relations on
  them** (declared type, access, throws), plus **modifiers and visibility**.

## 3. Gaps

### 3.1 Missing entities (→ #2, #7)

| Entity | Why it matters | Source |
|---|---|---|
| `Parameter` | declared type, annotations, `final`; receivers of accesses | TS query on formal parameters |
| `LocalVariable` | declared type, accesses | TS query on local declarations |
| `TypeParameter`, `Wildcard` | generics; bounds | TS query on type parameters |
| `AnnotationInstance` (and attributes) | frameworks (Spring, JUnit); today only the name is kept in `props` | TS query on annotations, resolved to the annotation type |
| `Comment` | documentation, TODO analyses | TS query on `line_comment`, `block_comment`, attached to the next declaration |
| `Lambda` | calls and accesses made inside lambdas | TS query on `lambda_expression` |
| `Exception` | `TException`: a class that is throwable | derived from `Inherits` |
| `ImplicitVariable`, `Initializer`, `AnnotationType` | completeness | TS |

These need a finer `Kind` table (#2): the closed 15-kind enum is replaced by
a table of kinds describing, for each, the traits it composes.

### 3.2 Missing relations (→ #8)

| Relation | Famix | Today | Gap |
|---|---|---|---|
| `declared_type` | `TEntityTyping` | none | variable, parameter, attribute, method return type to the type |
| `accesses` | `TAccess` (`isWrite`) | `References` | split read/write, targets that are variables |
| `throws` | `TWithExceptions` (declared, thrown, caught) | none | method to throwable, and the `throw`/`catch` sites |
| `overrides` | derived (`isOverriding`, `isOverridden`) | none | method to the method it overrides: from `Inherits` and signatures, or SCIP `relationships` |
| `receiver` of a call | `TInvocation.receiver` | not kept | site flag: receiver is `this`, `super`, a variable, a type |
| type arguments | `TConcretization` | none | `List<String>`: a use with arguments |
| annotation target | `TAnnotationInstance.annotatedEntity` | partly (`props`) | a relation from the annotated entity |

Site-level data (read vs write, receiver, `super` call, lambda body) is stored
as flags on the relation's sites, not as extra relations.

### 3.3 Out of scope

- Metrics (`TClassMetrics`, `TMethodMetrics`, `TLCOMMetrics`,
  `TCohesionCouplingMetrics`): analyses, they belong in `model-analysis`.
- Statement-level AST (`TWithStatements`), `UnknownVariable`, source groups.
- File system entities and tool concerns (`TFile`, `TFolder`, encodings).
- Other languages' traits until Java is complete (§4, #13).

## 4. Plan of epic #1

Goal: models at FamixJava level, built faster and on demand, composed from
traits, exportable to Moose as JSON from a separate crate.

### Layers

| Layer | Content | Cost |
|---|---|---|
| **L0 structure** | entities, `parent`, extents, modifiers, names. Tree-sitter only; local to one file | fast, no index |
| **L1 resolution** | relations that need symbol resolution (`Calls`, `declared_type`, `overrides`, `Inherits`): SCIP, else name-based best effort | needs the index |
| **L2 derived** | everything computable from L0 and L1 (`isTestCase`, `isOverriding`, `Exception`, metrics) | computed on demand, never stored |

L0 is per file, so it can be cached per file. L1 is the expensive layer and
is what the demand-driven session avoids when the question does not need it.

### Pieces

1. **Fine-grained `Kind` and a `Metamodel` table** in `model-core` (#2): each
   kind lists the traits it composes. The closed enum stays an enum of the
   *roles* analyses match on, the table adds the detail.
2. **`model-fm3`** (#3): reads an FM3 metamodel exported as JSON (§1). The
   FamixJava metamodel stops being hand-written knowledge and becomes an
   input.
3. **`model-traits`** (#6): a trait schema generated from the FM3 metamodel
   (properties, multivalued, container, derived, opposite) and `has_trait`
   queries over entities.
4. **Java entities and relations** (#7, #8): the gaps of §3, as tree-sitter
   queries and SCIP facts. Missing relations are flags on sites.
5. **Per-file fragments** (#9): each file produces a fragment keyed by its
   git blob (the cache already used for SCIP documents); the model is composed
   from fragments, so a changed file only recomputes its own fragment. This
   is also the incremental rebuild of design §10.
6. **`model-session`** (#10): a demand-driven import. A request names a
   *scope* (files, packages) and the *traits* it needs; the session runs
   only the layers those require.
7. **`model-moose`**, a separate crate (#11, #12): imports Moose JSON into
   Model (a differential oracle: the same Java sources through VerveineJ and
   through Model, compared by trait and property) and exports Model as Moose
   JSON. The export uses the FM3 table of #3, so it names classes and
   properties as Famix does. Moose-specific naming and ids stay out of
   `model-core`.
8. **Generalisation** (#13): the same traits for TypeScript, Python and
   Pharo, once Java works.

`model-core` stays language-neutral and unaware of Moose: Famix knowledge
lives in data (the FM3 table) and in the separate crate (design rule 6).
