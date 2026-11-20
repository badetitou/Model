use model_core::{
    EntityKind as K, Model, ModelBuilder, Provenance, Range, RelationKind as R, Value,
};

use crate::{
    ExtractOptions, ExtractStats, Extractor, Fragment, FragmentSet, compile, extract_sources,
    grammars,
};

fn model(sources: &[(&str, &str)]) -> (Model, ExtractStats) {
    let mut b = ModelBuilder::new();
    let st = extract_sources(&mut b, sources, &ExtractOptions::default()).unwrap();
    (b.build(), st)
}

/// `(from name, to key, sites)` of the relations of `kind`, sorted.
fn edges(m: &Model, kind: R) -> Vec<(String, String, usize)> {
    let mut v: Vec<_> = m
        .relations()
        .filter(|r| r.kind() == &kind)
        .map(|r| {
            (
                r.from().name().to_string(),
                r.to().key().to_string(),
                r.sites().len(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_query_compiles() {
    for g in grammars() {
        let c = compile(g).unwrap();
        assert!(!c.roles.is_empty(), "{}", g.language);
    }
}

const JAVA_A: &str = r#"package com.acme.app;

import java.util.List;
import java.util.*;
import static java.util.Collections.sort;
import com.acme.core.Base;

@Service
public class A extends Base implements Runnable, Comparable<A> {
    @Autowired
    private Repo repo, other;

    public A() { this(1); }
    A(int x) { }

    @Override
    public void run() {
        helper();
        helper(1);
        List<String> l = new java.util.ArrayList<>();
        new Inner();
    }

    private void helper() { run(); }
    private void helper(int x) { }

    static class Inner { }
}
"#;

const JAVA_BASE: &str = r#"package com.acme.core;

public abstract class Base {
    enum Mode { ON, OFF }
}

interface Repo extends java.io.Serializable { }
"#;

#[test]
fn java_entities_keys_and_nesting() {
    let (m, st) = model(&[("src/A.java", JAVA_A), ("src/Base.java", JAVA_BASE)]);
    assert_eq!(st.files_with_errors, 0);
    let key = |k: &str| format!("model java . . {k}");
    let a = m.by_key(&key("com/acme/app/A#")).unwrap();
    assert_eq!((a.kind(), a.file()), (K::Class, Some("src/A.java")));
    // The extent starts at the annotations.
    assert_eq!(a.range().map(|r| r.start_line), Some(7));
    assert_eq!(a.name_range(), Some(Range::new(8, 13, 8, 14)));
    let up: Vec<_> = a.ancestors().map(|e| (e.kind(), e.name())).collect();
    assert_eq!(
        up,
        [
            (K::Namespace, "app"),
            (K::Namespace, "acme"),
            (K::Namespace, "com")
        ]
    );
    let children: Vec<_> = a
        .children()
        .map(|e| {
            e.key()
                .trim_start_matches(&key("com/acme/app/A#"))
                .to_string()
        })
        .collect();
    assert_eq!(
        children,
        [
            "Service:",
            "repo.",
            "other.",
            "`<init>`().",
            "`<init>`(+1).",
            "run().",
            "helper().",
            "helper(+1).",
            "Inner#"
        ]
    );
    let ctor = m.by_key(&key("com/acme/app/A#`<init>`().")).unwrap();
    assert_eq!((ctor.kind(), ctor.name()), (K::Constructor, "A"));
    let mode = m.by_key(&key("com/acme/core/Base#Mode#OFF.")).unwrap();
    assert_eq!(mode.kind(), K::Constant);
    // Files are roots for declared namespaces.
    let file = m.by_key(&key("src/`A.java`/")).unwrap();
    assert_eq!((file.kind(), file.parent()), (K::File, None));
    assert_eq!(m.languages(), [model_core::LanguageId::new("java")]);
}

#[test]
fn java_props() {
    let (m, _) = model(&[("src/A.java", JAVA_A)]);
    let ann = |k: &str| {
        m.by_key(&format!("model java . . com/acme/app/{k}"))
            .unwrap()
            .props()
            .get("annotations")
            .cloned()
    };
    assert_eq!(ann("A#"), Some(Value::List(vec!["Service".into()])));
    assert_eq!(ann("A#run()."), Some(Value::List(vec!["Override".into()])));
    assert_eq!(ann("A#repo."), Some(Value::List(vec!["Autowired".into()])));
    assert_eq!(ann("A#helper()."), None);
}

#[test]
fn java_relations_resolved_by_name() {
    let (m, st) = model(&[("src/A.java", JAVA_A), ("src/Base.java", JAVA_BASE)]);
    let key = |k: &str| format!("model java . . {k}");
    assert_eq!(
        edges(&m, R::Inherits),
        [("A".into(), key("com/acme/core/Base#"), 1)]
    );
    assert_eq!(
        edges(&m, R::Implements),
        [
            ("A".into(), key("Comparable#"), 1),
            ("A".into(), key("Runnable#"), 1),
            ("Repo".into(), key("java/io/Serializable#"), 1),
        ]
    );
    // Single-type imports only (not `.*`, not static), from the file.
    assert_eq!(
        edges(&m, R::Imports),
        [
            ("A.java".into(), key("com/acme/core/Base#"), 1),
            ("A.java".into(), key("java/util/List#"), 1),
        ]
    );
    // Overloads share a name: the first declared wins, sites are kept.
    assert_eq!(
        edges(&m, R::Calls),
        [
            ("helper".into(), key("com/acme/app/A#run()."), 1),
            ("run".into(), key("com/acme/app/A#helper()."), 2),
        ]
    );
    assert_eq!(
        edges(&m, R::References),
        [
            ("run".into(), key("com/acme/app/A#Inner#"), 1),
            ("run".into(), key("java/util/ArrayList#"), 1),
        ]
    );
    let list = m.by_key(&key("java/util/List#")).unwrap();
    assert!(list.external());
    assert_eq!(
        list.parent().map(|p| (p.name(), p.external())),
        Some(("util", true))
    );
    assert!(
        m.relations()
            .all(|r| r.provenance() == Provenance::TreeSitter)
    );
    assert_eq!(st.unresolved, 0, "{st:?}");
    // The `this(1)` call is an explicit_constructor_invocation, not a method call.
    assert_eq!(st.calls, 3);
}

#[test]
fn relations_can_be_left_to_scip() {
    let mut b = ModelBuilder::new();
    let opts = ExtractOptions {
        relations: false,
        ..ExtractOptions::default()
    };
    let st = extract_sources(&mut b, &[("src/A.java", JAVA_A)], &opts).unwrap();
    assert_eq!((st.calls, st.references, st.imports), (0, 0, 0));
    let m = b.build();
    assert_eq!(m.relation_count(), 0);
    assert!(m.entities().all(|e| !e.external()));
}

const TS_SERVICE: &str = r#"import { Injectable } from '@angular/core';
import { Base } from './base';

@Injectable({ providedIn: 'root' })
export class UserService extends Base implements OnInit {
  @Input() name: string;
  private count = 0;

  constructor(private http: HttpClient) { super(); }

  ngOnInit(): void { this.load(); helper(); }

  load(): void { this.http.get('/u'); }
}

export interface User { id: number; greet(): string; }
export type Id = string;
export enum Role { Admin, User }
export const helper = () => new UserService(null);
function local() { return 1; }
"#;

#[test]
fn typescript_entities_under_their_file() {
    let (m, st) = model(&[("src/app/user.service.ts", TS_SERVICE)]);
    assert_eq!(st.files_with_errors, 0);
    let key = |k: &str| format!("model typescript . . src/app/`user.service.ts`/{k}");
    let file = m
        .by_key("model typescript . . src/app/`user.service.ts`/")
        .unwrap();
    assert_eq!(file.kind(), K::File);
    let up: Vec<_> = file.ancestors().map(|e| e.name()).collect();
    assert_eq!(up, ["app", "src"]);
    let tops: Vec<_> = file.children().map(|e| (e.kind(), e.name())).collect();
    assert_eq!(
        tops,
        [
            (K::Class, "UserService"),
            (K::Interface, "User"),
            (K::TypeAlias, "Id"),
            (K::Enum, "Role"),
            (K::Function, "helper"),
            (K::Function, "local"),
        ]
    );
    let svc = m.by_key(&key("UserService#")).unwrap();
    let members: Vec<_> = svc.children().map(|e| (e.kind(), e.name())).collect();
    assert_eq!(
        members,
        [
            (K::Field, "name"),
            (K::Field, "count"),
            (K::Constructor, "constructor"),
            (K::Method, "ngOnInit"),
            (K::Method, "load"),
        ]
    );
    assert!(m.by_key(&key("UserService#`<constructor>`().")).is_some());
    assert_eq!(
        m.by_key(&key("User#greet().")).map(|e| e.kind()),
        Some(K::Method)
    );
    assert_eq!(
        m.by_key(&key("User#id.")).map(|e| e.kind()),
        Some(K::Property)
    );
    assert_eq!(
        svc.props().get("decorators"),
        Some(&Value::List(vec!["Injectable".into()]))
    );
    assert_eq!(
        m.by_key(&key("UserService#name."))
            .unwrap()
            .props()
            .get("decorators"),
        Some(&Value::List(vec!["Input".into()]))
    );
}

#[test]
fn typescript_relations() {
    let (m, st) = model(&[("src/app/user.service.ts", TS_SERVICE)]);
    let ts = |k: &str| format!("model typescript . . {k}");
    let local = |k: &str| ts(&format!("src/app/`user.service.ts`/{k}"));
    assert_eq!(
        edges(&m, R::Inherits),
        [("UserService".into(), ts("Base#"), 1)]
    );
    assert_eq!(
        edges(&m, R::Implements),
        [("UserService".into(), ts("OnInit#"), 1)]
    );
    assert_eq!(
        edges(&m, R::Calls),
        [
            ("ngOnInit".into(), local("UserService#load()."), 1),
            ("ngOnInit".into(), local("helper()."), 1),
        ]
    );
    assert_eq!(
        edges(&m, R::References),
        [("helper".into(), local("UserService#"), 1)]
    );
    // `this.http.get` and the decorator calls have no target (`super()` is
    // not an identifier call).
    assert_eq!(st.unresolved, 3, "{st:?}");
}

#[test]
fn python_with_the_grammars_own_tags_query() {
    let src = "class Greeter:\n    def hello(self):\n        return self.name()\n\n    def name(self):\n        return 'x'\n\ndef main():\n    Greeter().hello()\n\nVERSION = 1\n";
    let (m, _) = model(&[("pkg/greet.py", src)]);
    let key = |k: &str| format!("model python . . pkg/`greet.py`/{k}");
    assert_eq!(m.by_key(&key("Greeter#")).map(|e| e.kind()), Some(K::Class));
    // A function directly in a class is a method.
    assert_eq!(
        m.by_key(&key("Greeter#hello().")).map(|e| e.kind()),
        Some(K::Method)
    );
    assert_eq!(
        m.by_key(&key("main().")).map(|e| e.kind()),
        Some(K::Function)
    );
    assert_eq!(
        m.by_key(&key("VERSION.")).map(|e| e.kind()),
        Some(K::Constant)
    );
    let calls = edges(&m, R::Calls);
    assert_eq!(
        calls,
        [
            ("hello".into(), key("Greeter#name()."), 1),
            ("main".into(), key("Greeter#hello()."), 1),
        ]
    );
}

#[test]
fn broken_sources_are_still_extracted() {
    let (m, st) = model(&[("A.java", "class A { void m() { int x = ; } }")]);
    assert_eq!(st.files_with_errors, 1);
    assert!(m.by_key("model java . . A#m().").is_some());
}

#[test]
fn deterministic() {
    let json = |m: &Model| {
        let mut s = String::new();
        for e in m.entities() {
            s.push_str(&format!("{:?}{:?}{:?}\n", e, e.parent(), e.range()));
        }
        for r in m.relations() {
            s.push_str(&format!("{r:?}\n"));
        }
        s
    };
    let srcs = [
        ("src/A.java", JAVA_A),
        ("src/Base.java", JAVA_BASE),
        ("x/u.ts", TS_SERVICE),
    ];
    assert_eq!(json(&model(&srcs).0), json(&model(&srcs).0));
}

#[test]
fn java_fine_kinds() {
    let src = r#"package p;
record Point(int x, int y) { }
@interface Marker { }
enum Mode { ON, OFF }
class C { }
"#;
    let (m, _) = model(&[("src/p/X.java", src)]);
    let kinds = |name: &str| {
        let e = m.entities().find(|e| e.name() == name).unwrap();
        (e.kind(), e.kind_name().to_string())
    };
    assert_eq!(kinds("Point"), (K::Class, "record".into()));
    assert_eq!(kinds("Marker"), (K::Interface, "annotation_type".into()));
    assert_eq!(kinds("ON"), (K::Constant, "enum_constant".into()));
    assert_eq!(kinds("Mode"), (K::Enum, "enum".into()));
    assert_eq!(kinds("C"), (K::Class, "class".into()));
    assert_eq!(m.of_kind(K::Class).count(), 2);
}

#[test]
fn java_inner_entities() {
    let src = r#"package p;
/** Doc. */
@Entity
class Box<T extends Number> {
    <U> void put(@Valid String s, int... rest) {
        // note
        int a = 1, b = 2;
        Runnable r = () -> { int a = 3; };
        java.util.function.Function<String, String> f = x -> x;
        for (String e : rest2) { }
        try { } catch (Exception ex) { }
    }
    void put(int s) { }
}
"#;
    let (m, _) = model(&[("src/p/Box.java", src)]);
    let keys: Vec<(String, String)> = m
        .entities()
        .filter(|e| {
            !matches!(
                e.kind_name(),
                "namespace" | "file" | "class" | "method" | "constructor"
            )
        })
        .map(|e| {
            let k = e.key().to_string();
            (
                e.kind_name().to_string(),
                k.splitn(5, ' ').last().unwrap().to_string(),
            )
        })
        .collect();
    let has = |kind: &str, key: &str| {
        assert!(
            keys.iter().any(|(k, s)| k == kind && s == key),
            "{kind} {key} missing in {keys:#?}"
        )
    };
    has("type_parameter", "p/Box#[T]");
    has("type_parameter", "p/Box#put().[U]");
    has("parameter", "p/Box#put().(s)");
    has("parameter", "p/Box#put().(rest)");
    has("parameter", "p/Box#put().(x)");
    has("parameter", "p/Box#put().(ex)");
    has("parameter", "p/Box#put(+1).(s)");
    has("local_variable", "p/Box#put().a.");
    has("local_variable", "p/Box#put().b.");
    has("local_variable", "p/Box#put().a+1.");
    has("local_variable", "p/Box#put().r.");
    has("local_variable", "p/Box#put().e.");
    has("annotation", "p/Box#Entity:");
    has("annotation", "p/Box#put().(s)Valid:");
    assert_eq!(keys.iter().filter(|(k, _)| k == "comment").count(), 2);
    let e = m.entities().find(|e| e.name() == "// note").unwrap();
    assert_eq!(e.parent().unwrap().name(), "put");
    assert_eq!(e.kind(), K::Variable);
}

fn json(sources: &[(&str, &str)], set: &FragmentSet) -> String {
    let _ = sources;
    let mut b = ModelBuilder::new();
    set.compose(&mut b, true);
    serde_json::to_string(&b.build()).unwrap()
}

fn fragments(sources: &[(&str, &str)]) -> FragmentSet {
    let ex = Extractor::new(&ExtractOptions::default()).unwrap();
    sources
        .iter()
        .filter_map(|(p, s)| ex.fragment(p, s.as_bytes()).unwrap())
        .collect()
}

const JAVA_B: &str = "package com.acme.app;\nclass B { void run() { new A().go(); } }";
const JAVA_B2: &str =
    "package com.acme.app;\nclass B { void run() { new A().go(); } void extra() {} }";

#[test]
fn fragments_round_trip_and_compose_to_the_direct_model() {
    let sources = [("a/A.java", JAVA_A), ("b/B.java", JAVA_B)];
    let (direct, _) = model(&sources);
    let set = fragments(&sources);
    let wire: Vec<Fragment> = set
        .iter()
        .map(|f| serde_json::from_str(&serde_json::to_string(f).unwrap()).unwrap())
        .collect();
    assert_eq!(wire, set.iter().cloned().collect::<Vec<_>>());
    let revived: FragmentSet = wire.into_iter().collect();
    assert_eq!(
        serde_json::to_string(&direct).unwrap(),
        json(&sources, &revived)
    );
}

#[test]
fn replacing_one_fragment_rebuilds_the_changed_project() {
    let before = [("a/A.java", JAVA_A), ("b/B.java", JAVA_B)];
    let after = [("a/A.java", JAVA_A), ("b/B.java", JAVA_B2)];
    let mut set = fragments(&before);
    let ex = Extractor::new(&ExtractOptions::default()).unwrap();
    set.insert(
        ex.fragment("b/B.java", JAVA_B2.as_bytes())
            .unwrap()
            .unwrap(),
    );
    let (direct, _) = model(&after);
    assert_eq!(serde_json::to_string(&direct).unwrap(), json(&after, &set));
    // Dropping a file's fragment removes its entities.
    set.remove("b/B.java");
    let (only_a, _) = model(&[("a/A.java", JAVA_A)]);
    assert_eq!(serde_json::to_string(&only_a).unwrap(), json(&after, &set));
}

#[test]
fn fragment_tag_follows_the_grammar() {
    let ex = Extractor::new(&ExtractOptions::default()).unwrap();
    let (j, t) = (ex.tag("A.java").unwrap(), ex.tag("a.ts").unwrap());
    assert_ne!(j, t);
    assert_eq!(ex.tag("A.java").unwrap(), j);
    assert!(ex.tag("notes.txt").is_none());
}

#[test]
fn java_declared_types_accesses_throws_overrides() {
    let (m, st) = model(&[
        ("p/I.java", "package p;\ninterface I { void run(); }\n"),
        ("p/E.java", "package p;\nclass E extends Exception {}\n"),
        (
            "p/A.java",
            r#"package p;
class A implements I {
    private int count;
    private E err, other;
    public void run() throws E {
        int n = count;
        this.count = n;
        count += 1;
        n++;
        helper(err);
        err.toString();
    }
    E helper(E e) { E tmp = e; return tmp; }
}
"#,
        ),
    ]);
    let key = |s: &str| format!("model java . . p/{s}");
    assert_eq!(
        edges(&m, R::DeclaredType),
        [
            ("e".into(), key("E#"), 1),
            ("err".into(), key("E#"), 1),
            ("helper".into(), key("E#"), 1),
            ("other".into(), key("E#"), 1),
            ("tmp".into(), key("E#"), 1),
        ]
    );
    assert_eq!(edges(&m, R::Throws), [("run".into(), key("E#"), 1)]);
    assert_eq!(
        edges(&m, R::Overrides),
        [("run".into(), key("I#run()."), 0)]
    );
    // `count`: read (n = count), write (this.count =), read+write (+=).
    let acc: Vec<_> = m
        .relations()
        .filter(|r| r.kind() == &R::Accesses && r.to().name() == "count")
        .collect();
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].provenance(), Provenance::TreeSitter);
    let mut flags: Vec<u8> = acc[0].site_flags().iter().map(|f| f.0).collect();
    flags.sort();
    assert_eq!(flags, [1, 2, 3]);
    // `n`: a local, written by n++ (read+write) and read in `this.count = n`.
    let n = m
        .relations()
        .find(|r| r.kind() == &R::Accesses && r.to().name() == "n")
        .unwrap();
    assert_eq!(n.to().kind_name(), "local_variable");
    assert_eq!(n.sites().len(), 2);
    // `err` is read as an argument and as a receiver.
    let err = m
        .relations()
        .find(|r| r.kind() == &R::Accesses && r.to().name() == "err")
        .unwrap();
    let mut f: Vec<u8> = err.site_flags().iter().map(|f| f.0).collect();
    f.sort();
    assert_eq!(f, [1, 5]);
    assert!(
        st.accesses >= 8 && st.overrides == 1 && st.throws == 1,
        "{st:?}"
    );
}

fn kind_of(m: &Model, key: &str) -> (String, K, Option<String>) {
    let e = m.by_key(key).unwrap_or_else(|| panic!("no {key}"));
    (
        e.kind_name().to_string(),
        e.kind(),
        e.kind_info().and_then(|k| k.fm3.clone()),
    )
}

#[test]
fn kinds_file_is_valid_and_traits_exist() {
    let kinds = crate::facts::kinds();
    assert!(kinds.len() >= 14, "kinds.json did not parse");
    for k in kinds {
        for t in &k.traits {
            assert!(
                model_traits::trait_def(t).is_some(),
                "{}: trait {t}",
                k.name
            );
        }
    }
}

#[test]
fn typescript_kinds_are_fine_kinds_with_traits() {
    let src = "export interface Shape { area(): number }\ntype Id = string;\nnamespace N { export const x = 1 }\nabstract class Base {}\nclass A extends Base implements Shape { area() { return 1 } }\nconst f = () => 1;\nenum E { A }\n";
    let (m, _) = model(&[("src/a.ts", src)]);
    let key = |k: &str| format!("model typescript . . src/`a.ts`/{k}");
    let fm3 = |c: &str| Some(format!("Famix-TypeScript-Entities.{c}"));
    assert_eq!(
        kind_of(&m, &key("Shape#")),
        ("ts.interface".into(), K::Interface, fm3("Interface"))
    );
    assert_eq!(
        kind_of(&m, &key("Id#")),
        ("ts.type_alias".into(), K::TypeAlias, fm3("TypeAlias"))
    );
    assert_eq!(kind_of(&m, &key("Base#")).0, "ts.abstract_class");
    assert_eq!(kind_of(&m, &key("f().")).0, "ts.arrow_function");
    assert_eq!(kind_of(&m, &key("A#")), ("class".into(), K::Class, None));
    assert_eq!(kind_of(&m, &key("E#")).0, "enum");
    assert_eq!(
        kind_of(&m, "model typescript . . src/`a.ts`/"),
        ("ts.module".into(), K::File, fm3("Module"))
    );
    // Archetype queries still see the fine kinds.
    assert_eq!(m.of_kind(K::Interface).count(), 1);
    let members = model_traits::mask(&["WithMembers", "WithInheritances"]);
    let mut names: Vec<_> = m.of_trait(members).map(|e| e.name().to_string()).collect();
    names.sort();
    assert_eq!(names, ["Base", "Shape"]);
}

#[test]
fn python_modules_parameters_and_superclasses() {
    let src = "class A(B, m.C, metaclass=M):\n    def f(self, x, y=1, *a):\n        return x\n";
    let (m, _) = model(&[("pkg/a.py", src), ("pkg/b.py", "class B: pass\n")]);
    let key = |k: &str| format!("model python . . pkg/`a.py`/{k}");
    assert_eq!(
        kind_of(&m, "model python . . pkg/`a.py`/").0,
        "python.module"
    );
    assert_eq!(kind_of(&m, &key("A#f().(x)")).0, "parameter");
    assert_eq!(kind_of(&m, &key("A#f().(y)")).0, "parameter");
    assert!(m.by_key(&key("A#f().(self)")).is_none());
    let sup: Vec<_> = edges(&m, R::Inherits).into_iter().map(|e| e.1).collect();
    assert!(sup.iter().any(|k| k.ends_with("`b.py`/B#")), "{sup:?}");
    assert_eq!(sup.len(), 2, "{sup:?}");
    let mods = model_traits::mask(&["Named", "WithMembers"]);
    assert!(
        m.by_key("model python . . pkg/`a.py`/")
            .unwrap()
            .has_trait(mods)
    );
}
