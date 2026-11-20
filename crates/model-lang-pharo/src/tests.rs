use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use model_core::{
    EntityKind, EntityRef, Model, ModelBuilder, Provenance, Range, RelationKind, Value,
};

use super::{PharoStats, Recv, extract, extract_sources, method_key, parse_source, type_key};

static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str, files: &[(&str, &str)]) -> Self {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("model-pharo-{tag}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (path, content) in files {
            let p = d.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        TempTree(d)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn parse_owned(files: &[(String, String)]) -> (Model, PharoStats, Vec<String>) {
    let mut b = ModelBuilder::new();
    let mut warnings = Vec::new();
    let stats = extract_sources(&mut b, files, &mut warnings);
    (b.build(), stats, warnings)
}

fn parse(files: &[(&str, &str)]) -> (Model, PharoStats, Vec<String>) {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(p, s)| ((*p).to_string(), (*s).to_string()))
        .collect();
    parse_owned(&owned)
}

fn one(path: &str, src: &str) -> (Model, PharoStats, Vec<String>) {
    parse(&[(path, src)])
}

/// Structure-like `Owner#selector` / `Owner class#selector` name.
fn logical(e: EntityRef<'_>) -> String {
    let owner = e.parent().map(|p| p.name().to_string()).or_else(|| {
        e.props().get("pharo.owner").and_then(|v| match v {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
    });
    let Some(owner) = owner else {
        return e.name().to_string();
    };
    let side = if matches!(e.props().get("pharo.classSide"), Some(Value::Bool(true))) {
        " class"
    } else {
        ""
    };
    format!("{owner}{side}#{}", e.name())
}

fn ent<'m>(m: &'m Model, key: &str) -> EntityRef<'m> {
    m.by_key(key)
        .unwrap_or_else(|| panic!("missing entity {key}"))
}

fn calls(m: &Model) -> Vec<(String, String)> {
    m.relations()
        .filter(|r| r.kind() == &RelationKind::Calls)
        .map(|r| (logical(r.from()), logical(r.to())))
        .collect()
}

fn targets(m: &Model, from_key: &str, kind: &RelationKind) -> Vec<String> {
    ent(m, from_key)
        .outgoing()
        .filter(|r| r.kind() == kind)
        .map(|r| logical(r.to()))
        .collect()
}

fn raw_sends(body: &str) -> Vec<(String, Recv)> {
    let src = format!("Class {{ #name : #C }}\nC >> m [ {body} ]\n");
    let mut b = ModelBuilder::new();
    let mut warnings = Vec::new();
    let f = parse_source(&mut b, "C.class.st", &src, "d", &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    f.sends.into_iter().map(|s| (s.selector, s.recv)).collect()
}

fn sel(v: &[(String, Recv)]) -> Vec<&str> {
    v.iter().map(|(s, _)| s.as_str()).collect()
}

/// Classes as `(name, superclass, classVars, methods source)`.
fn project(_tag: &str, classes: &[(&str, &str, &str, &str)]) -> (Model, PharoStats, Vec<String>) {
    let files: Vec<(String, String)> = classes
        .iter()
        .map(|(n, sup, cv, methods)| {
            let cv = if cv.is_empty() {
                String::new()
            } else {
                format!(", #classVars : [ '{cv}' ]")
            };
            let src = format!(
                "Class {{ #name : #{n}, #superclass : #{sup}{cv}, #package : 'App' }}\n{methods}\n"
            );
            (format!("src/App/{n}.class.st"), src)
        })
        .collect();
    parse_owned(&files)
}

fn snapshot(m: &Model) -> Vec<String> {
    let mut out: Vec<String> = m
        .entities()
        .map(|e| {
            format!(
                "E {} {:?} {} {:?} {:?} {:?} {} {} {:?}",
                e.key(),
                e.kind(),
                e.name(),
                e.file(),
                e.range(),
                e.name_range(),
                e.external(),
                e.parent().map_or("-", |p| p.key()),
                e.props()
            )
        })
        .collect();
    out.extend(m.relations().map(|r| {
        format!(
            "R {} {} {} {:?} {:?}",
            r.from().key(),
            r.kind().as_str(),
            r.to().key(),
            r.provenance(),
            r.sites()
        )
    }));
    out
}

const V3: &str = r#""
A comment with [brackets
"
Class {
 	#name : #OrderService,
 	#superclass : #Object,
 	#traits : 'TLogging',
 	#classTraits : 'TLogging classTrait',
 	#instVars : [ 'repo', 'log' ],
 	#package : 'MyApp-Core',
 	#tag : 'Services'
}

{ #category : #accessing }
OrderService >> place: anOrder at: aDate [
 	"closing ] in a comment"
 	| s |
 	s := 'it''s a ] string'.
 	$] = $[ ifTrue: [ ^ #Foo ].
 	^ Order new repo: OrderRepository default; yourself
]

{ #category : #'instance creation' }
OrderService class >> new [
 	^ super new init
]

OrderService >> + other [
 	^ Smalltalk at: #Missing ifAbsent: [ Widget new ]
]
"#;

#[test]
fn v3_class_and_methods() {
    let path = "src/MyApp-Core/OrderService.class.st";
    let (m, stats, warnings) = one(path, V3);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(stats.files, 1);
    assert_eq!(stats.classes, 1);
    assert_eq!(stats.methods, 3);

    let class = ent(&m, &type_key("OrderService").to_string());
    assert_eq!(class.kind(), EntityKind::Class);
    assert_eq!(class.name(), "OrderService");
    assert_eq!(class.file(), Some(path));
    assert_eq!(class.range().unwrap().start_line, 3);
    assert_eq!(
        class.props().get("pharo.package"),
        Some(&Value::Str("MyApp-Core-Services".into()))
    );
    assert_eq!(
        targets(&m, class.key(), &RelationKind::Inherits),
        ["Object"]
    );

    let method = ent(
        &m,
        &method_key("OrderService", false, "place:at:").to_string(),
    );
    assert_eq!(method.kind(), EntityKind::Method);
    assert_eq!(method.parent(), Some(class));
    assert_eq!(
        method.range(),
        Some(Range::new(14, 0, 20, 1)),
        "{:?}",
        method.range()
    );
    assert_eq!(method.name_range(), Some(Range::new(14, 16, 14, 34)));

    assert_eq!(
        method_key("OrderService", false, "place:at:").to_string(),
        "model pharo . . OrderService#`place:at:`()."
    );
    assert_eq!(
        method_key("OrderService", true, "new").to_string(),
        "model pharo . . OrderService#new(class)."
    );
    assert!(
        ent(&m, &method_key("OrderService", true, "new").to_string())
            .props()
            .contains_key("pharo.classSide")
    );
    assert!(
        !ent(&m, &method_key("OrderService", false, "+").to_string())
            .props()
            .contains_key("pharo.classSide")
    );

    let defined: Vec<String> = m
        .entities()
        .filter(|e| e.file().is_some())
        .map(logical)
        .collect();
    assert_eq!(defined.len(), 4, "{defined:?}");

    // Traits are parsed but deliberately inert: no trait entity or edge.
    assert!(m.by_key(&type_key("TLogging").to_string()).is_none());
    assert!(
        m.relations()
            .all(|r| r.provenance() == Provenance::Frontend)
    );
}

#[test]
fn body_references_ignore_kernel_globals() {
    let (m, _, warnings) = one("OrderService.class.st", V3);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        targets(
            &m,
            &method_key("OrderService", false, "place:at:").to_string(),
            &RelationKind::References
        ),
        ["Order", "OrderRepository"]
    );
    assert_eq!(
        targets(
            &m,
            &method_key("OrderService", false, "+").to_string(),
            &RelationKind::References
        ),
        ["Widget"]
    );
}

#[test]
fn header_fallbacks_traits_and_extensions() {
    let src = "Class {\n\t#name : 'Foo',\n\t#superclass : 'Bar',\n\t#category : 'App-Model'\n}\n\nFoo >> x [ ^ 1 ]\n";
    let (m, _, warnings) = one("Foo.class.st", src);
    assert!(warnings.is_empty());
    assert_eq!(
        ent(&m, &type_key("Foo").to_string())
            .props()
            .get("pharo.package"),
        Some(&Value::Str("App-Model".into()))
    );
    assert_eq!(
        targets(&m, &type_key("Foo").to_string(), &RelationKind::Inherits),
        ["Bar"]
    );

    let (m, stats, _) = one(
        "src/dir/Foo.class.st",
        "Class { #name : #Foo }\nFoo >> x [ ^ 1 ]",
    );
    assert_eq!(stats.classes, 1);
    assert_eq!(
        ent(&m, &type_key("Foo").to_string())
            .props()
            .get("pharo.package"),
        Some(&Value::Str("dir".into()))
    );

    let src = "Trait {\n\t#name : #TFoo,\n\t#package : 'App'\n}\n\nTFoo >> foo [ ^ Bar new ]\n";
    let (m, _, _) = one("TFoo.trait.st", src);
    assert_eq!(
        ent(&m, &type_key("TFoo").to_string()).kind(),
        EntityKind::Interface
    );
    assert!(targets(&m, &type_key("TFoo").to_string(), &RelationKind::Inherits).is_empty());

    let src = "Extension { #name : #String }\n\n{ #category : #'*MyApp' }\nString >> shout [\n\t^ self asUppercase, '!'\n]\n\nString class >> hello [ ^ Greeter new ]\n";
    let (m, stats, warnings) = one("src/MyApp-Ext/String.extension.st", src);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(stats.classes, 0);
    assert_eq!(stats.methods, 2);
    let shout = ent(&m, &method_key("String", false, "shout").to_string());
    assert_eq!(shout.parent(), None);
    assert_eq!(shout.range().unwrap().start_line, 3);
    assert_eq!(
        shout.props().get("pharo.package"),
        Some(&Value::Str("MyApp-Ext".into()))
    );
    assert!(
        m.by_key(&method_key("String", true, "hello").to_string())
            .is_some()
    );
}

#[test]
fn malformed_files_keep_partial_results_and_warn() {
    let src = "Class { #name : #Foo }\nFoo >> a [ ^ 1 ]\nFoo >> b [ ^ [ 2 ";
    let (m, stats, warnings) = one("Foo.class.st", src);
    assert!(!warnings.is_empty());
    assert_eq!(stats.methods, 2);
    assert!(
        m.by_key(&method_key("Foo", false, "a").to_string())
            .is_some()
    );

    let (m, stats, warnings) = one("x.st", "hello");
    assert!(!warnings.is_empty());
    assert_eq!(stats.files, 1);
    assert_eq!(m.entity_count(), 0);
}

#[test]
fn class_vars_and_pools_are_not_references() {
    let src = "Class {\n\t#name : #C,\n\t#superclass : #Object,\n\t#classVars : [ 'Counter' ],\n\t#pools : [ #Default ],\n\t#category : 'App'\n}\n\nC class >> next [ ^ Counter + Default + Other ]\n";
    let files = [
        ("src/App/C.class.st", src),
        (
            "src/App/C.extension.st",
            "Extension { #name : #C }\nC >> peek [ ^ Counter ]\n",
        ),
    ];
    let (m, _, warnings) = parse(&files);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        targets(
            &m,
            &method_key("C", true, "next").to_string(),
            &RelationKind::References
        ),
        ["Other"]
    );
    assert!(
        targets(
            &m,
            &method_key("C", false, "peek").to_string(),
            &RelationKind::References
        )
        .is_empty()
    );
    assert!(m.by_key(&type_key("Counter").to_string()).is_none());
    assert!(m.by_key(&type_key("Default").to_string()).is_none());
    assert_eq!(
        ent(&m, &type_key("C").to_string())
            .name_range()
            .map(|r| (r.start_line, r.start_col)),
        Some((1, 10))
    );
}

#[test]
fn project_graph() {
    let files = [
        ("src/.properties", "{ #format : #tonel }"),
        ("src/App-Core/package.st", "Package { #name : 'App-Core' }"),
        (
            "src/App-Core/Base.class.st",
            "Class { #name : #Base, #superclass : #Object, #category : 'App-Core' }\nBase >> go [ ^ Base new, Helper new, Morph new ]\n",
        ),
        (
            "src/App-Core/Helper.class.st",
            "Class { #name : #Helper, #superclass : #Base, #package : 'App-Core' }\n",
        ),
        (
            "src/App-Core/Base.extension.st",
            "Extension { #name : #Helper }\nHelper >> extra [ ^ 1 ]\n",
        ),
    ];
    let (m, stats, warnings) = parse(&files);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(stats.files, 3);
    assert_eq!(stats.classes, 2);
    assert_eq!(stats.methods, 2);

    let mut defined: Vec<String> = m
        .entities()
        .filter(|e| e.file().is_some())
        .map(logical)
        .collect();
    defined.sort();
    assert_eq!(defined, ["Base", "Base#go", "Helper", "Helper#extra"]);
    assert_eq!(
        ent(&m, &method_key("Helper", false, "extra").to_string())
            .parent()
            .map(|p| p.name()),
        Some("Helper")
    );

    let mut edges: Vec<(String, String, String)> = m
        .relations()
        .map(|r| {
            (
                logical(r.from()),
                r.kind().as_str().to_string(),
                logical(r.to()),
            )
        })
        .collect();
    edges.sort();
    assert_eq!(
        edges,
        [
            ("Base".into(), "inherits".into(), "Object".into()),
            ("Base#go".into(), "references".into(), "Helper".into()),
            ("Base#go".into(), "references".into(), "Morph".into()),
            ("Helper".into(), "inherits".into(), "Base".into()),
        ]
    );
}

#[test]
fn unary_binary_keyword_and_nesting() {
    let v = raw_sends("a foo bar. ^ a + b at: 1 put: x baz");
    assert_eq!(sel(&v), ["foo", "bar", "+", "at:put:", "baz"]);
    let v = raw_sends("self foo bar");
    assert_eq!(
        v,
        [("foo".into(), Recv::SelfRecv), ("bar".into(), Recv::Other)]
    );
    let v = raw_sends("^ self a: (x b: 1) b: y c + 2");
    assert_eq!(sel(&v), ["a:b:", "b:", "c", "+"]);
    assert_eq!(v[0].1, Recv::SelfRecv);
    assert_eq!(v[1].1, Recv::Other);
}

#[test]
fn cascade_reuses_receiver() {
    let v = raw_sends("Foo new x: 1; y; z: 2");
    assert_eq!(
        v,
        [
            ("new".into(), Recv::Class("Foo".into())),
            ("x:".into(), Recv::Other),
            ("y".into(), Recv::Other),
            ("z:".into(), Recv::Other),
        ]
    );
    let v = raw_sends("self a; b: 3; + 4");
    assert_eq!(sel(&v), ["a", "b:", "+"]);
    assert!(v.iter().all(|(_, r)| *r == Recv::SelfRecv));
    let v = raw_sends("Transcript show: 'x'; cr");
    assert!(
        v.iter()
            .all(|(_, r)| *r == Recv::Class("Transcript".into()))
    );
}

#[test]
fn blocks_assignments_and_braces() {
    let v = raw_sends(
        "| t | t := x ifTrue: [ self a ] ifFalse: [ :e | | u | e b. ^ self c ]. { self d. self e }",
    );
    assert_eq!(sel(&v), ["ifTrue:ifFalse:", "a", "b", "c", "d", "e"]);
}

#[test]
fn literals_comments_and_pragmas_are_not_sends() {
    let v = raw_sends(
        "<primitive: 1>\n\"self hidden\" ^ 'self str' size + #(self foo: bar) size + #sym + #at:put: + $a + #[1 2] size + 3.5 + -2 abs",
    );
    assert_eq!(
        sel(&v),
        [
            "size", "+", "size", "+", "+", "+", "+", "size", "+", "+", "abs"
        ]
    );
    let v = raw_sends("<script> <pragma: 1 other: 'x'> self a");
    assert_eq!(sel(&v), ["a"]);
    let v = raw_sends("x := #key. self foo. Bar baz");
    assert_eq!(
        v,
        [
            ("foo".into(), Recv::SelfRecv),
            ("baz".into(), Recv::Class("Bar".into())),
        ]
    );
    let v = raw_sends("self at: 'it''s' put: 3; foo");
    assert_eq!(
        v,
        [
            ("at:put:".into(), Recv::SelfRecv),
            ("foo".into(), Recv::SelfRecv),
        ]
    );
}

#[test]
fn range_of_a_call_edge() {
    let (m, stats, warnings) = project(
        "range",
        &[(
            "A",
            "Object",
            "",
            "A >> go [\n\t^ self at: 1 put: 2\n]\nA >> at: a put: b [ ^ 1 ]",
        )],
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(calls(&m), [("A#go".into(), "A#at:put:".into())]);
    assert_eq!(stats.sends_resolved, 1);
    assert_eq!(stats.sends_unresolved, 0);
    assert_eq!(stats.calls, 1);
    let call = m
        .relations()
        .find(|r| r.kind() == &RelationKind::Calls)
        .unwrap();
    assert_eq!(call.provenance(), Provenance::Frontend);
    assert_eq!(call.sites(), &[Range::new(2, 8, 2, 11)]);
}

#[test]
fn self_and_super_resolution() {
    let (m, stats, warnings) = project(
        "selfsuper",
        &[
            (
                "Base",
                "Object",
                "",
                "Base >> hello [ ^ 1 ]\nBase >> run [ ^ self hello ]\nBase class >> make [ ^ 1 ]",
            ),
            (
                "Sub",
                "Base",
                "",
                "Sub >> go [ ^ self hello + self nothing ]\nSub >> hello [ ^ super hello ]\nSub class >> make [ ^ super make + self make ]",
            ),
        ],
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = calls(&m);
    assert!(c.contains(&("Sub#go".into(), "Sub#hello".into())), "{c:?}");
    assert!(
        c.contains(&("Sub#hello".into(), "Base#hello".into())),
        "{c:?}"
    );
    assert!(
        c.contains(&("Base#run".into(), "Base#hello".into())),
        "{c:?}"
    );
    assert!(
        c.contains(&("Sub class#make".into(), "Base class#make".into())),
        "{c:?}"
    );
    assert_eq!(c.len(), 4, "{c:?}");
    assert_eq!(stats.sends_unresolved, 3, "{stats:?}");
}

#[test]
fn class_receiver_and_class_variables() {
    let (m, _, warnings) = project(
        "classrecv",
        &[
            (
                "Base",
                "Object",
                "",
                "Base class >> default [ ^ self new ]\nBase >> inst [ ^ 1 ]",
            ),
            ("Sub", "Base", "", ""),
            (
                "User",
                "Object",
                "Cache",
                "User >> a [ ^ Sub default ]\nUser >> b [ ^ Sub inst ]\nUser >> c [ ^ Cache default ]\nUser >> d [ ^ Missing default ]",
            ),
        ],
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = calls(&m);
    assert!(
        c.contains(&("User#a".into(), "Base class#default".into())),
        "{c:?}"
    );
    assert!(
        c.contains(&("User#c".into(), "Base class#default".into())),
        "{c:?}"
    );
    assert!(!c.iter().any(|(f, _)| f == "User#b"), "{c:?}");
    assert!(!c.iter().any(|(f, _)| f == "User#d"), "{c:?}");
}

#[test]
fn class_variable_is_not_a_class_receiver() {
    let (m, _, warnings) = project(
        "cvar",
        &[
            (
                "Other",
                "Object",
                "",
                "Other class >> shared [ ^ 1 ]\nOther class >> dup [ ^ 1 ]",
            ),
            ("Dup", "Object", "", "Dup class >> dup [ ^ 2 ]"),
            (
                "User",
                "Object",
                "Other",
                "User >> a [ ^ Other shared ]\nUser >> b [ ^ Other dup ]",
            ),
        ],
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = calls(&m);
    assert!(
        c.contains(&("User#a".into(), "Other class#shared".into())),
        "{c:?}"
    );
    assert!(!c.iter().any(|(f, _)| f == "User#b"), "{c:?}");
}

#[test]
fn unique_ambiguous_and_denylisted() {
    let (m, _, warnings) = project(
        "unique",
        &[
            (
                "A",
                "Object",
                "",
                "A >> only [ ^ 1 ]\nA >> dup [ ^ 1 ]\nA >> value [ ^ 1 ]",
            ),
            (
                "B",
                "Object",
                "",
                "B >> dup [ ^ 2 ]\nB >> go [ ^ x only + x dup + x value ]\nB >> ext [ ^ x shout ]",
            ),
        ],
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = calls(&m);
    assert!(c.contains(&("B#go".into(), "A#only".into())), "{c:?}");
    assert_eq!(c.iter().filter(|(f, _)| f == "B#go").count(), 1, "{c:?}");

    let files = [
        (
            "src/App/String.extension.st",
            "Extension { #name : #String }\nString >> shout [ ^ 1 ]\n",
        ),
        (
            "src/App/B.class.st",
            "Class { #name : #B, #superclass : #Object, #package : 'App' }\nB >> go [ ^ x shout ]\n",
        ),
    ];
    let (m, _, _) = parse(&files);
    assert_eq!(calls(&m), [("B#go".into(), "String#shout".into())]);
}

#[test]
fn disk_and_memory_agree() {
    let files: Vec<(String, String)> = vec![
        (
            "src/Demo-Model/DmItem.class.st".into(),
            "Class {\n\t#name : 'DmItem',\n\t#superclass : 'Object',\n\t#category : 'Demo-Model'\n}\n\n{ #category : 'x' }\nDmItem >> price [\n\t^ 1\n]\n".into(),
        ),
        (
            "src/Demo-UI/Object.extension.st".into(),
            "Extension { #name : 'Object' }\n\n{ #category : '*Demo-UI' }\nObject >> dmTotal [\n\t^ DmItem new price\n]\n".into(),
        ),
        (
            "src/Demo-UI/package.st".into(),
            "Package { #name : 'Demo-UI' }\n".into(),
        ),
        ("README.md".into(), "x".into()),
    ];
    let tree = TempTree::new(
        "mem",
        &files
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect::<Vec<_>>(),
    );
    let mut b = ModelBuilder::new();
    let mut disk_warnings = Vec::new();
    let disk_stats = extract(&mut b, tree.path(), &mut disk_warnings).unwrap();
    let disk = b.build();

    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let (mem, mem_stats, mem_warnings) = parse(&refs);

    assert!(disk_warnings.is_empty(), "{disk_warnings:?}");
    assert_eq!(mem_warnings, disk_warnings);
    assert_eq!(mem_stats, disk_stats);
    assert!(!snapshot(&mem).is_empty());
    assert_eq!(snapshot(&mem), snapshot(&disk));
    assert_eq!(
        calls(&mem),
        [("Object#dmTotal".into(), "DmItem#price".into())]
    );
}

#[test]
fn pharo_kinds_carry_famix_smalltalk_traits() {
    let (m, _, _) = parse(&[
        (
            "Demo/Foo.class.st",
            "Class { #name : #Foo, #superclass : #Object }\n\n{ #category : #x }\nFoo >> run [ ^ self ]\n\n{ #category : #x }\nFoo class >> make [ ^ self new ]\n",
        ),
        ("Demo/TFoo.trait.st", "Trait { #name : #TFoo }\n"),
        (
            "Demo/Foo.extension.st",
            "Extension { #name : #Foo }\n\n{ #category : #'*Demo' }\nFoo >> extra [ ^ 1 ]\n",
        ),
    ]);
    let info = |key: &str| {
        let e = m.by_key(key).unwrap();
        (
            e.kind_name().to_string(),
            e.kind(),
            e.kind_info().and_then(|k| k.fm3.clone()),
        )
    };
    assert_eq!(
        info(&type_key("Foo").to_string()),
        (
            "pharo.class".into(),
            EntityKind::Class,
            Some("Famix-Smalltalk-Entities.Class".into())
        )
    );
    assert_eq!(info(&type_key("TFoo").to_string()).0, "pharo.trait");
    assert_eq!(
        info(&method_key("Foo", false, "run").to_string()).0,
        "pharo.method"
    );
    // Class-side is a property of a method, not a kind.
    assert_eq!(
        info(&method_key("Foo", true, "make").to_string()).0,
        "pharo.method"
    );
    assert_eq!(
        info(&method_key("Foo", false, "extra").to_string()).0,
        "pharo.extension_method"
    );
    let calls = model_traits::mask(&["Invocation", "Access"]);
    let mut names: Vec<_> = m.of_trait(calls).map(|e| e.name().to_string()).collect();
    names.sort();
    assert_eq!(names, ["extra", "make", "run"]);
    let heritage = model_traits::mask(&["WithInheritances"]);
    assert_eq!(m.of_trait(heritage).count(), 1);
}
