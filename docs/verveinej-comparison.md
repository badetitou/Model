# VerveineJ vs Model: Famix export comparison on VerveineJ's own sources (with dependencies)

> **Status.** A measurement, not a specification: made on the branch state of
> 2026-11-20 (tip `fb6d913`, see below) on VerveineJ's own sources, first without
> its dependency jars (Appendix A) and then with them (main comparison). Model
> has changed since; re-measure before quoting a figure. Paths and scratchpad
> scripts mentioned below belong to the session that made it and are not in the
> repository. See [usage.md](usage.md) and [examples.md](examples.md) (scenario f)
> for how to run the import and the diff yourself.

Measurement only: no Model or VerveineJ code was changed. Branch `claude/verveine-compare-632296`
(tip `fb6d913`). All artefacts and scripts live in the session scratchpad, not in the repo.

## Main comparison: VerveineJ WITH dependencies

### D.1 Setup
Same image `ghcr.io/evref-bl/verveinej:v4.1.4`, same sources (232 files), same options
(`-format json -alllocals -anchor assoc`), plus `-v $HOME/.vcmp-dep:/dependency` (the entrypoint adds
`-autocp /dependency`). 3 s, **12 351 553 B**, **81 104 objects** (source-only: 12 216 333 B, 80 990).
Model exports are the same files as in the source-only run (not rerun).

Jars present (15, all copied from `~/.gradle/caches/modules-2`, nothing downloaded, no token):
`org.eclipse.jdt.core-3.47.0` (the version in `build.gradle.kts`), `ecj-3.46.100`, `fame-java-v1.0.6`
(found locally in the Gradle cache, so the GitHub Packages artifact was not needed), `akuhn-util-r28011`
(`app/lib`), `commons-codec-1.20.0`, `org.eclipse.core.contenttype-3.9.900`, `core.jobs-3.15.900`,
`core.runtime-3.35.0`, `equinox.app-1.7.600`, `equinox.common-3.21.0`, `equinox.preferences-3.12.100`,
`org.eclipse.osgi-3.24.300`, `org.eclipse.text-3.14.800`, `org.osgi.service.prefs-1.1.2`,
`osgi.annotation-8.0.1`.

Missing or approximate: `commons-codec` 1.13 (build file) is not cached, 1.20.0 used; the transitive Eclipse
platform jars are the newest cached versions, not a Gradle-resolved set; the JDK in the image is 17 while the
project targets 25; the Famix entity/trait classes come from the sources themselves, so no separate famix jar
was needed. Consequence: remaining VerveineJ stubs are mostly the unresolved bits below (4 entries).
Pharo: the new JSON loaded into a fresh, unregistered `FamixJavaModel` without error (81 104 objects,
Class 402, Method 4162, Invocation 6391, Inheritance 415, Access 10 858, ParametricClass 17,
Concretization 2059).

### D.2 What changed in VerveineJ itself (source-only → with dependencies)
| Item | Source-only | With deps |
|---|---:|---:|
| Class (stubs) | 1138 (914) | 402 (178) |
| ParametricClass (stubs) | 132 (132) | 17 (17) |
| Interface (stubs) | 152 (25) | 165 (38) |
| AnnotationType | 1394 | 6 |
| Method (stubs) | 4284 (686) | 4162 (564) |
| ParametricMethod | 27 | 29 |
| Attribute (stubs) | 466 (7) | 481 (22) |
| Package (stubs) | 38 (18) | 54 (34) |
| TypeParameter | 68 | 72 |
| Exception | 13 | 15 |
| Inheritance / ParametricInheritance | 1260 / 136 | 415 / 139 |
| Implementation / Parametric | 181 / 17 | 197 / 21 |
| Invocation / ParametricInvocation | 6470 / 133 | 6391 / 214 |
| Access | 10 833 | 10 858 |
| Reference / Parametric | 1051 / 315 | 1079 / 315 |
| EntityTyping / Parametric | 12 752 / 1314 | 13 166 / 1320 |
| Concretization | 1848 | 2059 |
| AnnotationInstanceAttribute | 40 | 2537 |
| AnnotationTypeAttribute | 1 | 7 |
| Wildcard / PrimitiveType | 92 / 6 | 92 / 6 |

Library types now sit in real packages: stub types by top package are `org.eclipse` 134, `java.lang` 44,
`java.util` 36, `java.io` 22, `ch.akuhn` 21, `java.security` 4 (before: `org.moosetechnology` 1536,
`fr.inria` 845 of guessed stubs). Still unresolved: `<Default Package>.TNamedEntity`, `TCanBeStub`,
`<StubMethodContainer>`, one type with no package. Inheritance towards stubs fell from 1218 to 376 pairs.
Real (non-stub) entities are unchanged: 355 types, 3617 methods (234 in anonymous classes, 13
initializers), identical keys.

### D.3 Model vs VerveineJ (with deps), per FM3 class
| FM3 class | VerveineJ deps | Model TS | Model SCIP | Note |
|---|---:|---:|---:|---|
| Package | 54 | 47 | 52 | stubs 34 / 27 / 32 |
| Class | 402 | 359 | 316 | non-stub 224 / 107 / 107 |
| Interface | 165 | 127 | 127 | |
| Enum / EnumValue | 4 / 28 | 4 / 28 | 4 / 28 | |
| AnnotationType | 6 | 6 | 6 | equal count (V qualifies names, Model not) |
| AnnotationInstance | 2065 | 2065 | 2065 | |
| AnnotationInstanceAttribute | 2537 | 0 | 0 | |
| ParametricClass + Interface | 17 + 14 | 0 | 0 | |
| TypeParameter | 72 | 20 | 20 | |
| Method / ParametricMethod | 4162 / 29 | 3585 / 19 | 4131 / 19 | non-stub 3617+ / 3585 / 3585 |
| Attribute | 481 | 459 | 736 | stubs 22 / 0 / 277 |
| Parameter / LocalVariable | 2520 / 958 | 2542 / 936 | 2542 / 936 | |
| Comment | 1491 | 1247 | 1247 | |

Association totals (with deps): Invocation 6391+214 vs TS 2534 vs SCIP 8540; Access 10 858 vs 7507 vs 0;
Reference 1079+315 vs 691 vs 8354; Inheritance 415+139 vs 185 vs 587; Implementation 197+21 vs 124 vs 530;
EntityTyping 13 166+1320 vs 4403 vs 0; Concretization 2059 vs 0; Import 0 vs 1347 vs 0.
`receiver` V 4351/6391 (68 %), `signature` 100 %.

Pairs between the 3403 source entities present on both sides, and towards external entities:
| Kind | V internal | TS match (V-only / M-only) | SCIP match (V-only / M-only) | V→external | TS→external | SCIP→external |
|---|---:|---:|---:|---:|---:|---:|
| Invocation | 2757 | 2050 = 74.4 % (707 / 394) | 2751 = 99.8 % (6 / 0) | 3501 | 0 | 5436 |
| Inheritance | 178 | 178 (0 / 0) | 178 (0 / +376 method-level) | 376 | 7 | 33 |
| Implementation | 124 | 124 (0 / 0) | 124 (0 / +406) | 94 | 0 | 0 |
| Reference | 906 | 11 (895 / 20) | 769 = 84.9 % (137 / 2709) | 290 | 649 | 4439 |
| Access | 1997 | 965 = 48.3 % (1032 / 0) | 0 | 8693 | 6452 | 0 |
| EntityTyping | 407 | 406 (1 / 1) | 0 | 7696 | 3992 | 0 |

Stub type names (distinct): V 269, TS 259, SCIP 215; shared with V: TS 112 (43 %), SCIP 204
(94.9 % of SCIP's). With dependencies the SCIP variant lines up with VerveineJ on external types
(`java.lang.*`, `org.eclipse.jdt.core.dom.*`, `ch.akuhn.fame.*`); tree-sitter still has 145 stubs without
package and unqualified annotation types. Anchors, comments and method matching are unchanged (methods
2439/2439 identical offsets, types 218/238, attributes 0/459; 3352/3617 methods exact, 18 arity, 247
V-only; 1246/1247 comments, 245 header comments skipped).

### D.4 Ranked differences, re-evaluated (source-only → with deps)
1. **Missing generic layer, persists, larger.** V: 17+14 parametric types (was 132+13, mostly stub
   artefacts), Concretization 2059 (was 1848), ParametricEntityTyping 1320, Wildcard 92, TypeParameter 72.
   Model: none, 20 TypeParameter.
2. **Names/signatures/modifiers absent, persists.** `signature` 4191 methods (V) vs 0, `visibility` V
   5253 entities vs 0, cyclomatic complexity 2868 vs 0; also `isAbstract/isFinal/isClassSide/synchronized`.
3. **Anonymous classes and initializers, persists unchanged:** 117 classes, 234 methods, 13 initializers
   (not a source-only artefact).
4. **Catch parameters as Parameters, persists unchanged:** 18 arity mismatches, 116 extra parameters.
5. **Associations.** Persist: Access 48.3 % (SCIP 0), Reference 11/906 (TS), EntityTyping SCIP 0. Improved
   because V got more precise: Invocation SCIP match 96.1 % → 99.8 %, TS 71.6 % → 74.4 % (V's unresolved
   internal calls dropped from 2862 to 2757 pairs). External supertypes: V 1218 → 376 pairs, so
   Model's coverage is now 7/376 (1.9 %) in TS and 33/376 (8.8 %) in SCIP, instead of 0.6 % / 2.7 %.
6. **Stubs: mostly a source-only artefact, now resolved.** V's 914 stub classes → 178, 1394 AnnotationType →
   6, guessed packages disappeared. Remaining Model gap: TS stubs without package (145), unqualified
   annotation types, `isStub` on methods (TS 0 vs V 564); SCIP matches V's stub names at 95 %.
7. **AnnotationInstanceAttribute:** 2537 in V (was 40, artefact of unresolved annotations), 0 in Model,
   so this is a larger real gap than first reported (annotation arguments are lost).
8. **Anchors:** unchanged (artefact/convention only).
9. **Artefacts only:** `--root` file names, diff tooling on Parameter/LocalVariable, `this` vs `self`
   (ImplicitVariable 1839 vs 1244), 245 header comments, and the SCIP index provenance.

### D.5 Next steps (revised)
Same list as below, with priority 1 for signature/modifiers, generics, anonymous classes, catch variables,
annotation attribute values, and for TS qualifying stub types through imports (SCIP already does). The
tree-sitter external supertypes and invocations towards library methods are now measurable targets (376 and
3501 V pairs).

### D.6 Reproduce the with-dependencies run
```bash
C=~/.gradle/caches/modules-2/files-2.1; D=$HOME/.vcmp-dep; mkdir -p $D $HOME/.vcmp/{src,out}
# copy the jars listed in D.1 (non-javadoc, non-sources) from $C into $D, plus app/lib/akuhn-util-r28011.jar
cp -R /Users/benoit.verhaeghe/Documents/Moosetechnology/VerveineJ/app/src/main/java/. $HOME/.vcmp/src/
docker run --rm -e JAVA_XMX=-Xmx4G -v $HOME/.vcmp/src:/src -v $D:/dependency -v $HOME/.vcmp/out:/out \
  ghcr.io/evref-bl/verveinej:v4.1.4 -format json -alllocals -anchor assoc -o /out/verveine-dep.json
target/release/examples/import $HOME/.vcmp/out/verveine-dep.json --root $HOME/.vcmp/src --diff model-ts.json
```
(my scripts are the scratchpad `dep/*.py`, copies of the earlier ones pointing at `verveine-dep.json`).

---

# Appendix A: VerveineJ source-only run (first comparison)

Everything below was measured with VerveineJ run **without** `/dependency` jars. Sections 4.6 and parts of
4.5 are inflated by that choice; see D.2 and D.4 for the corrected figures.

## 1. Setup

**Target.** `/Users/benoit.verhaeghe/Documents/Moosetechnology/VerveineJ` (commit `ec7f6263`). It is a
**Gradle** project (not Maven; `app/build.gradle.kts`, JDK 25 toolchain, GitHub-token-protected FameJava
dependency). Analysed sources: `app/src/main/java` only, **232 files, 28 428 lines**, copied unchanged to
the scratchpad (`src/`). Tests (192 files) were left out.

**VerveineJ side.**
- Image `ghcr.io/evref-bl/verveinej:v4.1.4` (VerveineJ.jar of 14 Jan 2026, JDK 17), via the
  VerveineJ-Docker entrypoint, which appends `-autocp /dependency .`.
- Command (sources copied under `$HOME` because Colima does not share `/private/tmp`):
  `docker run --rm -e JAVA_XMX=-Xmx4G -v <src>:/src -v <out>:/out ghcr.io/evref-bl/verveinej:v4.1.4 -format json -alllocals -anchor assoc -o /out/verveine-src.json`
- **Source-only**: no `/dependency` jars (no JDT, Fame, JDK model). A with-dependencies run was not done;
  this inflates V's stub count (see 4.7).
- 3 s wall, **12 216 333 B**, 80 990 JSON objects.

**Model side** (`cargo build --release`, examples of this branch).
- tree-sitter only: `build <src> --no-scip --no-cache --json model-ts.json` (0.35 s, 11 916 entities), then
  `export model-ts.json moose-ts.json --root <src>` (0.38 s). `moose-ts.json` **11 493 376 B**, 53 007 objects.
- SCIP: `scip-java` is not installed and the Gradle build cannot run here (JDK 25 + token). I used the
  **pre-existing, untracked `index.scip`** found at the root of the VerveineJ checkout (7.2 MB, 9 Nov, produced
  by scip-java; I did not produce it, its freshness is only inferred: no source file is newer).
  Layout: scratchpad `scip-root/` = `app/src/main/java` + `index.scip` + a copy of `build.gradle.kts` (needed
  for the Gradle marker). `build <scip-root> --reindex never --no-cache --json model-scip.json`
  (0.17 s; 89.4 % of definitions mapped, 22 361 sites), exported with `--root scip-root`.
  `moose-scip.json` **14 035 800 B**, 59 379 objects.
- Integrated diff: `import verveine-src.json --root <src> --diff model-{ts,scip}.json`
  (outputs `diff-ts.txt`, `diff-scip.txt`).
- Moose check: the Pharo image has FamixJava. A fresh `FamixJavaModel new importFromJSONStream:` loaded all
  three files without error (80 990 / 53 007 / 59 379 objects; Class 1138 / 359 / 316; Method 4284 / 3585 /
  4131; Invocation 6470 / 2534 / 8540; Inheritance 1260 / 185 / 587; Access 10 833 / 7507 / 0). Models were
  not registered; no image code touched.

Matching in my own analysis (`load.py`, `qn.py`, `cmp.py`, `assoc.py`, `anch*.py` in the scratchpad): qualified
name for types/packages/attributes; `(owner qn, name, #Parameter)` for methods; `(method key, name)` for
locals. Associations are compared as pairs of those keys.

## 2. Counts per FM3 class (Moose JSON objects)

| FM3 class | VerveineJ | Model TS | Model SCIP | Note |
|---|---:|---:|---:|---|
| Package | 38 | 47 | 52 | stubs 18 / 27 / 32 |
| Class | 1138 | 359 | 316 | non-stub 224 / 107 / 107 |
| Interface | 152 | 127 | 127 | non-stub 127 / 127 / 127 |
| Enum / EnumValue | 4 / 28 | 4 / 28 | 4 / 28 | equal |
| AnnotationType | 1394 | 6 | 6 | V duplicates stubs (22 distinct names) |
| AnnotationInstance | 2065 | 2065 | 2065 | equal |
| Exception | 13 | 1 | 0 | |
| ParametricClass / ParametricInterface | 132 / 13 | 0 / 0 | 0 / 0 | |
| PrimitiveType / Wildcard / Type | 6 / 92 / 1 | 0 | 0 | |
| TypeParameter | 68 | 20 | 20 | |
| Method | 4284 | 3585 | 4131 | non-stub 3590 / 3585 / 3585 |
| ParametricMethod | 27 | 19 | 19 | |
| Attribute | 466 | 459 | 736 | 7 / 0 / 277 stubs |
| Parameter | 2520 | 2542 | 2542 | see 4.4 |
| LocalVariable | 958 | 936 | 936 | |
| ImplicitVariable | 1839 | 1244 | 1106 | named `this` (V) vs `self` (Model) |
| Comment | 1491 | 1247 | 1247 | |
| AnnotationInstanceAttribute | 40 | 0 | 0 | |
| IndexedFileAnchor | 27 909 | 23 527 | 28 033 | |
| SourceLanguage | 1 | 0 | 0 | |

## 3. Associations

| Association | VerveineJ | Model TS | Model SCIP |
|---|---:|---:|---:|
| Invocation (+Parametric) | 6470 (+133) | 2534 | 8540 |
| Access | 10 833 | 7507 | 0 |
| Reference (+Parametric) | 1051 (+315) | 691 | 8354 |
| Inheritance (+Parametric) | 1260 (+136) | 185 | 587 |
| Implementation (+Parametric) | 181 (+17) | 124 | 530 |
| EntityTyping (+Parametric) | 12 752 (+1314) | 4403 | 0 |
| Concretization | 1848 | 0 | 0 |
| Import | 0 | 1347 | 0 |
| throws (thrown/declared/caught lists) | 430 / 2 / 13 | 0 / 2 / 0 | 0 |

Pair-level comparison restricted to the 3403 source entities present on both sides
(rest = endpoints that are stubs/external or anonymous):

| Kind | V internal | Model-TS internal | match | V-only | Model-only | Model-SCIP internal | match |
|---|---:|---:|---:|---:|---:|---:|---:|
| Invocation | 2862 | 2444 | 2050 (71.6 % of V) | 812 | 394 | 2751 | 2751 (96.1 %) |
| Inheritance | 178 | 178 | 178 | 0 | 0 | 554 | 178 (+376 method-level) |
| Implementation | 124 | 124 | 124 | 0 | 0 | 530 | 124 (+406 method-level) |
| Reference | 906 | 31 | 11 | 895 | 20 | 3478 | 769 (84.9 %), 2709 extra |
| Access | 1997 | 965 | 965 (48.3 %) | 1032 | 0 | 0 | 0 |
| EntityTyping | 407 | 407 | 406 | 1 | 1 | 0 | 0 |

Pairs towards stubs/external types: Invocation V 3394 vs TS 0 (TS has no external method targets;
SCIP 5436); Inheritance V 1218 vs TS 7 vs SCIP 33; Access V 8668 vs TS 6452; Typing V 7276 vs TS 3992.
Other facts: `candidates` filled for 100 % of invocations on all sides; `receiver` V 68 % / TS 73 % /
SCIP 19 %; `signature` V 100 % / Model 0 %; `previous` chains V only (8790 Access, 4205 Invocation);
Access pairs also differ for `isWrite` (V 1593 set, TS 801).

## 4. Ranked differences

Class: (1) Model misses, (2) Model extra, (3) modelled differently, (4) artefact, (5) TS-vs-SCIP.

### 4.1 Missing generic/parametric layer (1), high
V: 132 ParametricClass, 13 ParametricInterface, 27 ParametricMethod, 1848 Concretization, 1314
ParametricEntityTyping, 136+17+133+315 Parametric Inheritance/Implementation/Invocation/Reference, 92
Wildcard, 68 TypeParameter (50 stub). Model: 20 TypeParameter, 19 ParametricMethod, nothing else. Generic
types used as superclass/field/variable type are therefore plain types. Moose queries on type arguments
return nothing.

### 4.2 Names, signatures and modifiers absent (1), high
Method `signature` (4284 in V, 0 in Model); `visibility` (V 4284 methods, 1136/1138 classes, 465
attributes; Model 0 everywhere); `isAbstract` 39, `isFinal` 59, `isClassSide` 110, `isSynchronized` 5 (all 0
in Model); `cyclomaticComplexity` (2849 methods + 19 parametric) and `numberOfStatements` (2848): 0.
`thrownExceptions` 430 / `caughtExceptions` 14: 0. Neither side fills `numberOfLinesOfCode`. Consequence:
Moose metrics, visibility and signature-based navigation do not work on the Model export; method identity
in Moose often relies on `signature`.

### 4.3 Anonymous classes missing, their methods hoisted (1/3), high
117 anonymous classes (`Owner.method._Anonymous(Super)`, Class in V) do not exist in Model (non-stub
types: V 355, Model 238, the 238 names are identical). V has 234 methods inside them; Model has 234
methods with no V counterpart, which is consistent with them being attached to the enclosing class
(counts equal; not individually verified). V also has 13 `<Initializer>` methods Model lacks. Effect on
methods: 3352 exact matches of 3617 real V methods (92.7 %), 18 name-matches with different arity, 247
V-only (234 + 13).

### 4.4 Catch parameters modelled as method Parameters (3), medium
Source: `EntityDictionary.asException(TType fmxType)` contains `catch(ConcurrentModificationException e)`.
V: 1 parameter + `e` as LocalVariable (plus `caughtExceptions`). Model: Parameters `fmxType, e` (arity 2),
creating 18 arity mismatches among matched methods (23 `catch` clauses in the sources) and 116 Model-only
parameters that V does not have. In the integrated diff, Parameter/LocalVariable/TypeParameter show 0 on
the Model side: the diff reads Model entities, and parameters/locals exist only in the export (4).

### 4.5 Associations: Access, Invocation, Reference differ strongly in TS (1/3/5), high
- Access: TS 7507 vs V 10 833; only 965 of 1997 internal pairs match (48.3 %). V-only 4421 pairs, of
  which 2898 target variables owned by a method (locals, parameters, implicit `this`). TS emits no Access
  to implicit variables. SCIP: **0 Access** (SCIP carries no field reads/writes distinction).
- Invocation: TS resolves by name only; 71.6 % of V's internal pairs, 0 to external methods (V 3394), 394
  Model-only pairs (mostly overload/arity ambiguity, e.g. `ensureFamixEntity` → `getEntityByKey`).
  SCIP: 96.1 % of V's internal pairs, with 5436 pairs to external methods. `receiver` only 19 % with SCIP.
- Reference: V = type use from a method (`new X`, cast, `instanceof`, class literal, e.g.
  `ensureFamixException → Exception` ×3). TS Reference 691 with only 11 of 906 internal pairs matching
  (Model emits e.g. `addFamixInvocation → Invocation`, the type of a local); SCIP emits 8354 (every
  mention), 85 % of V's pairs but 2709 extra.
- Inheritance/Implementation: internal pairs are identical (178 / 124). TS finds 7 of 1218 V pairs to
  external supertypes (0.6 %); SCIP finds 33 (2.7 %) plus 376/406 method-level extras (SCIP encodes
  method overrides as Inheritance/Implementation; the export drops them in some paths, see
  "relations not exported" in the export report, but 782 remain, wrong archetype: (3)).
- EntityTyping: TS 4403 vs V 12 752; 406/407 internal pairs match (99.8 %) but external types are mostly
  missing (3992 vs 7276); SCIP **0** (no declared types in SCIP-linked relations).
- Model has `Import` (1347), V has none (V 4.1.4 emits no Import): (2).

### 4.6 Stubs and external types (3/5), medium
V (source-only) creates 914 stub Class, 132 ParametricClass, 1394 AnnotationType (22 distinct names, one
stub per use, e.g. 720 `famixjavaentities.FameProperty`), 686 stub Methods. Names are guessed from
wildcard imports in the current package (e.g. `fr.inria.verveine.extractor.java.ASTNode` for
`org.eclipse.jdt.core.dom.ASTNode`), `<Default Package>.<StubMethodContainer>` and `<ImplicitVarType>`.
Model TS: 252 stub classes, **145 with no package** (`ASTNode`), 72 under `org.eclipse`, 6 stub annotation
types without package (`FameProperty`, `Override`), `isStub` set on 0 methods. SCIP: stubs get real
packages (`org.eclipse` 123, `java.lang` 26, ...), 546 stub methods, 277 stub attributes (`X.class`,
`Array.length`), but the 6 annotation types remain unqualified. Neither side matches the other on stubs
(V-only 381 / Model-only 231 stub type names in TS); this comparison is dominated by V's source-only
guesses, so it is partly an artefact (4).

### 4.7 Anchors (3), medium, mostly fine
Method anchors: 2439/2439 unique-key matches identical (start, end offsets, 100 %); parametric methods
16/16. Types: 218/238 identical, 20 start later in Model (V includes leading Javadoc/annotations/modifiers,
e.g. `VerveineJParser` V 1053 vs 1954). Attributes: 0/459 identical: V anchors the declarator
(`key`: 1144-1153), Model the whole field declaration (1129-1154). Model also anchors EnumValue (28/28, V 0),
AnnotationInstance (2065/2065, V 0), Inheritance/Implementation/Import (V 0). `fileName`: V `./fr/…`
(relative to the parse dir), Model the absolute path when `--root` is absolute (with SCIP root:
`main/java/...`, i.e. relative to the project root): file names are not directly comparable (4).
Comments: 1246 of 1247 Model comments have an identical anchor in V. V has 245 more: the per-file header
comment (e.g. `// Automagically generated code…`, anchor off by one char), which Model deliberately skips
(export report: "comment 245 not exported"). V attaches comments to Interface (127) and Attribute (64),
Model to Method and Class only.

### 4.8 Other
- ImplicitVariable: V `this` (1839), Model `self` (1244 TS / 1106 SCIP): Smalltalk naming (3), and 595
  fewer.
- Packages: V 38, Model 47: Model adds the chain of imported packages (`ch`, `ch.akuhn`, …) as stubs; V adds
  `<Default Package>`, `java.lang.constant`, `java.lang.invoke` (3).
- Exception type: V 13 (stubs, used with `throws`), Model 1.
- Attribute `isClassSide` 44, `isFinal` 42: absent in Model.
- AnnotationInstanceAttribute 40: missing (1, low).
- Not a difference: `overrides` (derived in Famix, V has no objects either).

## 5. Artefacts only
- `--root` file names (absolute vs `./relative` vs `main/java/...`).
- Integrated diff: parameters, locals, type parameters are unmapped on the Model side; comments 1491 left
  unmapped; stub packages/classes matched by guessed qualified names; V's 1394 duplicated AnnotationType
  stubs.
- V's source-only guessed stub packages; a with-dependencies run would change 4.6 (not measured).
- SCIP index provenance (pre-existing, from the user's earlier run).

## 6. Concrete next steps for Model (nothing written now)
1. Export `signature`, `visibility`, modifiers and class-side/final/abstract flags (data already in the
   tree-sitter query side; closes 4.2).
2. Model anonymous and local classes as Class entities (V naming `Owner.method._Anonymous(Super)`), and
   initializer blocks; stop hoisting their methods.
3. Catch clause variables → LocalVariable + caughtExceptions, not Parameter.
4. Generic layer: Parametric*, TypeParameter binding, Wildcard, Concretization, ParametricEntityTyping.
5. Declared types and field/variable Access with the SCIP variant (the `tree-sitter + SCIP` merge currently
   loses EntityTyping and Access entirely); Access to implicit variables; fix receiver coverage.
6. Reference semantics: type use (`new`, cast, `instanceof`, class literal) instead of declared types, and do
   not export SCIP method-level overrides as Inheritance/Implementation.
7. Stub naming: qualify stubs through imports (including wildcard) for TS; `isStub` on methods; one stub
   per annotation type; `this` instead of `self`; cyclomatic complexity and statements count.
8. Align anchor conventions (attribute declarator, type start) and make `--root` produce relative
   `fileName`s so diffs are direct; extend `diff` to match Parameter/LocalVariable and stubs by shape.

## 7. Reproduce
```bash
SP=<scratchpad>; mkdir -p ~/.vcmp/src ~/.vcmp/out
cp -R /Users/benoit.verhaeghe/Documents/Moosetechnology/VerveineJ/app/src/main/java/. ~/.vcmp/src/
docker run --rm -e JAVA_XMX=-Xmx4G -v ~/.vcmp/src:/src -v ~/.vcmp/out:/out \
  ghcr.io/evref-bl/verveinej:v4.1.4 -format json -alllocals -anchor assoc -o /out/verveine-src.json
cargo build --release -p model-factory --example build -p model-moose --examples
B=target/release/examples
$B/build ~/.vcmp/src --no-scip --no-cache --json $SP/model-ts.json
$B/export $SP/model-ts.json $SP/moose-ts.json --root ~/.vcmp/src
# SCIP: root = copy of app/src/main/java under app/src/main + index.scip + build.gradle.kts
$B/build $SP/scip-root --reindex never --no-cache --json $SP/model-scip.json
$B/export $SP/model-scip.json $SP/moose-scip.json --root $SP/scip-root
$B/import ~/.vcmp/out/verveine-src.json --root ~/.vcmp/src --diff $SP/model-ts.json
```
