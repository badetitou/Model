use model_core::{
    EntityKind as K, Model, ModelBuilder, Provenance, Range, RelationKind as R, Value,
};

use crate::{ExtractOptions, ExtractStats, compile, extract_sources, grammars};

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
