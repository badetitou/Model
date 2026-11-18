use model_core::{EntityKind as K, LanguageId, Model, ModelBuilder, Range, RelationKind as R};
use model_scip::{Definition, Reference, ScipDocument, ScipIndex, ScipRelationship, ScipSymbol};

use crate::{LinkOptions, LinkStats, from_scip, link};

fn r(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range::new(sl, sc, el, ec)
}

fn pfx(s: &str) -> String {
    format!("semanticdb maven m 1 {s}")
}

enum O {
    Def(&'static str, Range),
    DefIn(&'static str, Range, Range),
    Ref(&'static str, Range),
    Import(&'static str, Range),
}

/// Index of `(path, occurrences)`; symbols are prefixed with [`pfx`] unless
/// they already have a scheme.
struct Ix(ScipIndex);

impl Ix {
    fn new() -> Self {
        Ix(ScipIndex::default())
    }

    fn sym(&mut self, s: &str) -> model_scip::SymbolId {
        let full = if s.contains(' ') {
            s.to_string()
        } else {
            pfx(s)
        };
        self.0.intern(&full).unwrap()
    }

    fn doc(mut self, path: &str, language: &str, occs: Vec<O>) -> Self {
        let mut d = ScipDocument {
            relative_path: path.into(),
            language: language.into(),
            ..ScipDocument::default()
        };
        for o in occs {
            match o {
                O::Def(s, range) => d.definitions.push(Definition {
                    symbol: self.sym(s),
                    range,
                    enclosing: None,
                }),
                O::DefIn(s, range, enclosing) => d.definitions.push(Definition {
                    symbol: self.sym(s),
                    range,
                    enclosing: Some(enclosing),
                }),
                O::Ref(s, range) | O::Import(s, range) => {
                    let import = matches!(o, O::Import(..));
                    d.references.push(Reference {
                        symbol: self.sym(s),
                        range,
                        import,
                    })
                }
            }
        }
        self.0.documents.push(d);
        self
    }

    fn info(
        mut self,
        s: &str,
        kind: Option<K>,
        display: Option<&str>,
        implements: &[&str],
    ) -> Self {
        let relationships = implements
            .iter()
            .map(|t| ScipRelationship {
                symbol: self.sym(t),
                is_implementation: true,
                is_reference: false,
                is_type_definition: false,
                is_definition: false,
            })
            .collect();
        let id = self.sym(s);
        self.0.symbols.insert(
            id,
            ScipSymbol {
                display_name: display.map(Into::into),
                kind,
                enclosing_symbol: None,
                relationships,
            },
        );
        self
    }
}

/// Tree-sitter-like entity: `(key, kind, name, file, range, name_range, parent)`.
fn ts(b: &mut ModelBuilder, e: (&str, K, &str, &str, Range, Range, Option<&str>)) {
    let (key, kind, name, file, range, nr, parent) = e;
    let id = b.entity(&format!("model java . . {key}"), kind, name);
    b.set_source(id, file, range, Some(nr));
    if let Some(p) = parent {
        let p = b.get(&format!("model java . . {p}")).unwrap();
        b.set_parent(id, Some(p));
    }
}

fn edges(m: &Model, kind: R) -> Vec<(String, String, usize)> {
    let mut v: Vec<_> = m
        .relations()
        .filter(|rel| rel.kind() == &kind)
        .map(|rel| {
            (
                rel.from().name().to_string(),
                rel.to().key().to_string(),
                rel.sites().len(),
            )
        })
        .collect();
    v.sort();
    v
}

fn fixture() -> (ModelBuilder, ScipIndex) {
    let mut b = ModelBuilder::new();
    for e in [
        (
            "p/A#",
            K::Class,
            "A",
            "A.java",
            r(0, 0, 9, 1),
            r(0, 6, 0, 7),
            None,
        ),
        (
            "p/A#<init>().",
            K::Constructor,
            "A",
            "A.java",
            r(2, 2, 2, 12),
            r(2, 2, 2, 3),
            Some("p/A#"),
        ),
        (
            "p/A#run().",
            K::Method,
            "run",
            "A.java",
            r(4, 2, 6, 3),
            r(4, 7, 4, 10),
            Some("p/A#"),
        ),
        (
            "p/B#",
            K::Class,
            "B",
            "B.java",
            r(0, 0, 5, 1),
            r(0, 6, 0, 7),
            None,
        ),
        (
            "p/B#go().",
            K::Method,
            "go",
            "B.java",
            r(2, 2, 4, 3),
            r(2, 7, 2, 9),
            Some("p/B#"),
        ),
        (
            "p/I#",
            K::Interface,
            "I",
            "I.java",
            r(0, 0, 3, 1),
            r(0, 10, 0, 11),
            None,
        ),
        (
            "p/I#run().",
            K::Method,
            "run",
            "I.java",
            r(1, 2, 1, 14),
            r(1, 7, 1, 10),
            Some("p/I#"),
        ),
        (
            "p/C#",
            K::Class,
            "C",
            "C.java",
            r(0, 0, 5, 1),
            r(0, 6, 0, 7),
            None,
        ),
        (
            "p/C#go().",
            K::Method,
            "go",
            "C.java",
            r(2, 2, 4, 3),
            r(2, 7, 2, 9),
            Some("p/C#"),
        ),
    ] {
        ts(&mut b, e);
    }
    let ix = Ix::new()
        .doc(
            "A.java",
            "java",
            vec![
                O::Def("p/A#", r(0, 6, 0, 7)),
                O::Def("p/A#`<init>`().", r(2, 2, 2, 3)),
                O::Def("p/A#run().", r(4, 7, 4, 10)),
                O::Def("p/A#run().(x)", r(4, 11, 4, 12)), // parameter: noise
                O::Ref("p/I#", r(0, 20, 0, 21)),
            ],
        )
        .doc(
            "B.java",
            "java",
            vec![
                O::Def("p/B#", r(0, 6, 0, 7)),
                O::Def("p/B#go().", r(2, 7, 2, 9)),
                O::Ref("p/A#`<init>`().", r(3, 8, 3, 9)),
                O::Ref("p/A#", r(3, 8, 3, 9)),
                O::Ref("p/A#run().", r(3, 12, 3, 15)),
                O::Ref("p/A#run().", r(3, 30, 3, 33)),
                O::Ref("ext/Foo#bar().", r(3, 16, 3, 19)),
                O::Ref("ext/Baz#", r(3, 20, 3, 23)),
                O::Ref("ext/Imported#", r(8, 7, 8, 15)), // outside every entity
            ],
        )
        // No entity in this file: its references are ignored.
        .doc("Imp.java", "java", vec![O::Ref("p/A#", r(0, 7, 0, 8))])
        .doc(
            "I.java",
            "java",
            vec![
                O::Def("p/I#", r(0, 10, 0, 11)),
                O::Def("p/I#run().", r(1, 7, 1, 10)),
            ],
        )
        .doc(
            "C.java",
            "java",
            vec![
                O::Def("p/C#", r(0, 6, 0, 7)),
                O::Def("p/C#go().", r(2, 7, 2, 9)),
                O::Ref("p/I#run().", r(3, 4, 3, 7)),
            ],
        )
        .info("p/A#", None, None, &["p/I#"])
        .info("p/A#run().", None, None, &["p/I#run()."]);
    (b, ix.0)
}

#[test]
fn links_onto_syntactic_entities() {
    let (mut b, ix) = fixture();
    let st = link(&mut b, &ix, &LinkOptions::default());
    assert_eq!((st.mapped, st.lifted, st.unmapped), (9, 0, 0), "{st:?}");
    let m = b.build();

    // Matched entities take the SCIP symbol as key.
    assert_eq!(m.by_key(&pfx("p/A#")).map(|e| e.name()), Some("A"));
    assert!(m.by_key("model java . . p/A#").is_none());

    let calls = edges(&m, R::Calls);
    assert!(
        calls.contains(&("go".into(), pfx("p/A#run()."), 2)),
        "{calls:?}"
    );
    assert!(calls.contains(&("go".into(), pfx("p/A#`<init>`()."), 1)));
    assert!(calls.contains(&("go".into(), pfx("ext/Foo#bar()."), 1)));
    assert!(calls.contains(&("go".into(), pfx("p/I#run()."), 1)));
    // Derived facts are not linking: no call from C#go to A#run.
    assert_eq!(calls.len(), 4, "{calls:?}");

    let uses = edges(&m, R::References);
    assert_eq!(
        uses,
        [
            ("A".into(), pfx("p/I#"), 1),
            ("go".into(), pfx("ext/Baz#"), 1),
            ("go".into(), pfx("p/A#"), 1),
        ]
    );
    assert_eq!(
        edges(&m, R::Implements),
        [
            ("A".into(), pfx("p/I#"), 0),
            ("run".into(), pfx("p/I#run()."), 0)
        ]
    );

    // External entities, with their ancestors.
    let bar = m.by_key(&pfx("ext/Foo#bar().")).unwrap();
    assert!(bar.external());
    assert_eq!(bar.kind(), K::Method);
    let up: Vec<_> = bar
        .ancestors()
        .map(|e| (e.kind(), e.key().to_string()))
        .collect();
    assert_eq!(
        up,
        [(K::Class, pfx("ext/Foo#")), (K::Namespace, pfx("ext/"))]
    );
    assert!(m.by_key(&pfx("ext/Imported#")).is_none());

    assert_eq!(
        st,
        LinkStats {
            mapped: 9,
            external: 4, // ext/, ext/Foo#, ext/Foo#bar()., ext/Baz#
            references: 9,
            unattributed: 1,
            calls: 5,
            uses: 3,
            implements: 2,
            ..LinkStats::default()
        }
    );
}

/// scip-java puts method definitions on the annotation line; members
/// without an entity are lifted to their owner; references to project
/// symbols without entity are dropped, not made external.
#[test]
fn annotation_line_lifting_and_project_refs() {
    let mut b = ModelBuilder::new();
    for e in [
        (
            "C#",
            K::Class,
            "C",
            "C.java",
            r(0, 0, 10, 1),
            r(0, 13, 0, 14),
            None,
        ),
        (
            "D#",
            K::Class,
            "D",
            "D.java",
            r(0, 0, 3, 1),
            r(0, 6, 0, 7),
            None,
        ),
        (
            "C#m().",
            K::Method,
            "m",
            "C.java",
            r(2, 2, 5, 3),
            r(3, 9, 3, 10),
            Some("C#"),
        ),
    ] {
        ts(&mut b, e);
    }
    let ix = Ix::new()
        .doc(
            "C.java",
            "java",
            vec![
                O::Def("C#", r(0, 13, 0, 14)),
                O::Def("C#m().", r(2, 2, 2, 6)),
                O::Def("C#`<init>`().", r(0, 13, 0, 14)),
                O::Def("C#x.", r(1, 6, 1, 7)),
                O::Ref("C#m().", r(8, 4, 8, 5)),
            ],
        )
        .doc(
            "D.java",
            "java",
            vec![
                O::Def("D#", r(0, 6, 0, 7)),
                O::Ref("C#`<init>`().", r(1, 4, 1, 5)),
                O::Ref("C#x.", r(2, 4, 2, 5)),
                O::Ref("T#", r(2, 8, 2, 9)),
            ],
        )
        .doc("T.java", "java", vec![O::Def("T#", r(0, 6, 0, 7))]);
    let st = link(&mut b, &ix.0, &LinkOptions::default());
    assert_eq!(
        (st.mapped, st.lifted, st.unmapped, st.dropped),
        (3, 2, 1, 1),
        "{st:?}"
    );
    let m = b.build();
    assert_eq!(edges(&m, R::Calls), [("C".into(), pfx("C#m()."), 1)]);
    // Lifted targets are references, both sites on one relation.
    assert_eq!(edges(&m, R::References), [("D".into(), pfx("C#"), 2)]);
    assert!(m.entities().all(|e| !e.external()));
}

fn ts_index() -> ScipIndex {
    let a = "scip-typescript npm app 1.0 src/`a.ts`/A#";
    let run = "scip-typescript npm app 1.0 src/`a.ts`/A#run().";
    let x = "scip-typescript npm app 1.0 src/`a.ts`/A#x.";
    let lib = "scip-typescript npm lib 2.0 `index.d.ts`/Lib#";
    let make = "scip-typescript npm lib 2.0 `index.d.ts`/Lib#make().";
    Ix::new()
        .doc(
            "src/a.ts",
            "TypeScriptReact",
            vec![
                O::Import(lib, r(0, 9, 0, 12)),
                O::DefIn(a, r(1, 13, 1, 14), r(1, 0, 8, 1)),
                O::DefIn(run, r(2, 2, 2, 5), r(2, 2, 4, 3)),
                O::Def(
                    "scip-typescript npm app 1.0 src/`a.ts`/A#run().(p)",
                    r(2, 6, 2, 7),
                ),
                O::Ref(x, r(3, 4, 3, 5)),
                O::Ref(make, r(3, 10, 3, 14)),
                O::Ref(make, r(3, 20, 3, 24)),
                O::Def(x, r(5, 2, 5, 3)),
            ],
        )
        .info(a, Some(K::Class), None, &[lib])
        .info(run, Some(K::Method), Some("run"), &[])
        .info(lib, Some(K::Interface), Some("Lib"), &[])
        .0
}

#[test]
fn builds_a_model_from_scip_alone() {
    let (b, st) = from_scip(&ts_index(), None);
    assert_eq!(
        st,
        LinkStats {
            created: 3,
            external: 3, // `index.d.ts`/, Lib#, Lib#make().
            references: 4,
            calls: 2,
            uses: 1,
            imports: 1,
            implements: 1,
            ..LinkStats::default()
        }
    );
    let m = b.build();
    let ts = LanguageId::new("typescript");
    let a = m
        .by_key("scip-typescript npm app 1.0 src/`a.ts`/A#")
        .unwrap();
    assert_eq!((a.kind(), a.name()), (K::Class, "A"));
    assert_eq!(a.range(), Some(r(1, 0, 8, 1)));
    assert_eq!(a.name_range(), Some(r(1, 13, 1, 14)));
    assert_eq!(a.language(), Some(&ts));
    let up: Vec<_> = a.ancestors().map(|e| (e.kind(), e.name())).collect();
    assert_eq!(up, [(K::Namespace, "a.ts"), (K::Namespace, "src")]);
    let x = m
        .by_key("scip-typescript npm app 1.0 src/`a.ts`/A#x.")
        .unwrap();
    assert_eq!((x.kind(), x.parent()), (K::Field, Some(a)));

    let file = m.by_key("model typescript . . `src/a.ts`/").unwrap();
    assert_eq!((file.kind(), file.name()), (K::File, "a.ts"));
    assert_eq!(file.range(), Some(r(0, 0, 9, 0)));
    assert_eq!(
        edges(&m, R::Imports),
        [(
            "a.ts".into(),
            "scip-typescript npm lib 2.0 `index.d.ts`/Lib#".into(),
            1
        )]
    );
    assert_eq!(
        edges(&m, R::Calls),
        [(
            "run".into(),
            "scip-typescript npm lib 2.0 `index.d.ts`/Lib#make().".into(),
            2
        )]
    );
    assert_eq!(
        edges(&m, R::References),
        [(
            "run".into(),
            "scip-typescript npm app 1.0 src/`a.ts`/A#x.".into(),
            1
        )]
    );
    assert_eq!(
        edges(&m, R::Implements),
        [(
            "A".into(),
            "scip-typescript npm lib 2.0 `index.d.ts`/Lib#".into(),
            0
        )]
    );
    let lib = m
        .by_key("scip-typescript npm lib 2.0 `index.d.ts`/Lib#")
        .unwrap();
    assert!(lib.external());
    assert_eq!(lib.kind(), K::Interface);
}

#[test]
fn deterministic() {
    let json = |m: &Model| serde_json::to_string(m).unwrap();
    let a = from_scip(&ts_index(), None).0.build();
    let b = from_scip(&ts_index(), None).0.build();
    assert_eq!(json(&a), json(&b));
    let (mut b1, ix) = fixture();
    let (mut b2, _) = fixture();
    link(&mut b1, &ix, &LinkOptions::default());
    link(&mut b2, &ix, &LinkOptions::default());
    assert_eq!(json(&b1.build()), json(&b2.build()));
}

#[test]
fn language_override_and_normalisation() {
    assert_eq!(
        crate::normalize_language("TypeScriptReact"),
        Some(LanguageId::new("typescript"))
    );
    assert_eq!(
        crate::normalize_language("C++"),
        Some(LanguageId::new("cpp"))
    );
    assert_eq!(crate::normalize_language(""), None);
    assert_eq!(
        crate::language_of_path("src/A.java"),
        Some(LanguageId::new("java"))
    );
    assert_eq!(crate::language_of_path("Makefile"), None);
    let (b, _) = from_scip(&ts_index(), Some(LanguageId::new("ts")));
    let m = b.build();
    assert!(m.by_key("model ts . . `src/a.ts`/").is_some());
    assert_eq!(m.languages(), [LanguageId::new("ts")]);
}
