use model_core::{Model, ModelBuilder, Provenance, RelationKind, Value};
use model_treesitter::{ExtractOptions, extract_sources};

use crate::{
    JavaStats, PROP_ENDPOINTS, PROP_RPC_CALLS, PROP_RPC_SERVICE, enrich_sources, rpc_relation,
};

fn build(sources: &[(&str, &str)], generated: &[(&str, &str)]) -> (Model, JavaStats) {
    let mut b = ModelBuilder::new();
    extract_sources(&mut b, sources, &ExtractOptions::default()).unwrap();
    let stats = enrich_sources(&mut b, sources, generated).unwrap();
    (b.build(), stats)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::List(l)) => l
            .iter()
            .filter_map(|x| match x {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Endpoints of the method named `method` of type `ty` (simple names).
fn eps(m: &Model, ty: &str, method: &str) -> Vec<String> {
    let e = m
        .entities()
        .find(|e| e.name() == method && e.parent().is_some_and(|p| p.name() == ty))
        .unwrap_or_else(|| panic!("no {ty}.{method}"));
    strings(e.props().get(PROP_ENDPOINTS))
}

fn one(src: &str) -> Model {
    build(&[("A.java", src)], &[]).0
}

#[test]
fn spring_prefix_and_method() {
    let m = one(r#"@RestController @RequestMapping("/articles/{slug}")
class A { @GetMapping("comments/") void a() {} @PostMapping void b() {} void c() {} }"#);
    assert_eq!(eps(&m, "A", "a"), ["GET /articles/{slug}/comments"]);
    assert_eq!(eps(&m, "A", "b"), ["POST /articles/{slug}"]);
    assert!(eps(&m, "A", "c").is_empty());
}

#[test]
fn endpoints_are_entities_under_their_handler() {
    let m = one(r#"@RequestMapping("/a")
class A { @GetMapping("/x") void h() {} @PostMapping("/x") void i() {} }"#);
    let mut named: Vec<(String, String, String)> = m
        .entities()
        .filter(|e| e.kind_name() == crate::HTTP_ENDPOINT)
        .map(|e| {
            let prop = |k: &str| match e.props().get(k) {
                Some(Value::Str(s)) => s.to_string(),
                _ => String::new(),
            };
            (e.name().to_string(), prop("http.verb"), prop("http.path"))
        })
        .collect();
    named.sort();
    assert_eq!(
        named,
        [
            ("GET /a/x".into(), "GET".into(), "/a/x".into()),
            ("POST /a/x".into(), "POST".into(), "/a/x".into()),
        ]
    );
    let from_method = m.relations().filter(|r| {
        r.kind() == &RelationKind::Other("http.endpoints".into())
            && r.from().kind_name() == "method"
            && r.to().kind_name() == crate::HTTP_ENDPOINT
    });
    assert_eq!(from_method.count(), 2);
}

#[test]
fn spring_arrays_and_request_method() {
    let m = one(r#"class A {
  @GetMapping(path = {"/x", "y"}) void a() {}
  @RequestMapping(value = "/z", method = {RequestMethod.PUT, DELETE}) void b() {}
  @RequestMapping("/any") void c() {}
}"#);
    assert_eq!(eps(&m, "A", "a"), ["GET /x", "GET /y"]);
    assert_eq!(eps(&m, "A", "b"), ["PUT /z", "DELETE /z"]);
    assert_eq!(eps(&m, "A", "c"), ["ANY /any"]);
}

#[test]
fn unknown_request_method_gives_no_endpoint() {
    let m = one(r#"class A {
  @RequestMapping(value = "/h", method = RequestMethod.HEAD) void a() {}
  @RequestMapping(value = "/c", method = Consts.M) void b() {}
  @RequestMapping(value = "/m", method = {RequestMethod.GET, RequestMethod.OPTIONS}) void c() {}
}"#);
    assert!(eps(&m, "A", "a").is_empty());
    assert!(eps(&m, "A", "b").is_empty());
    assert_eq!(eps(&m, "A", "c"), ["GET /m"]);
}

#[test]
fn jaxrs_path_and_verb() {
    let m = one(r#"@Path("/api") class A {
  @GET @Path("/items/{id}") void a() {}
  @POST void b() {}
  @Path("/nope") void c() {}
}"#);
    assert_eq!(eps(&m, "A", "a"), ["GET /api/items/{id}"]);
    assert_eq!(eps(&m, "A", "b"), ["POST /api"]);
    assert!(eps(&m, "A", "c").is_empty());
}

#[test]
fn http_clients_serve_no_endpoint() {
    let m = one(r#"@FeignClient(name = "x") @RequestMapping("/api")
interface F { @GetMapping("/a") void a(); }
@HttpExchange("/b") interface H { @GetMapping("/c") void c(); }"#);
    assert!(eps(&m, "F", "a").is_empty());
    assert!(eps(&m, "H", "c").is_empty());
}

#[test]
fn constant_path_is_unknown() {
    let m =
        one(r#"class A { @GetMapping(Consts.X) void a() {} @GetMapping("a" + "b") void b() {} }"#);
    assert_eq!(eps(&m, "A", "a"), ["GET ?"]);
    assert_eq!(eps(&m, "A", "b"), ["GET ?"]);
}

#[test]
fn endpoints_attach_to_the_right_overload_and_nested_type() {
    let m = one(r#"class A {
  @GetMapping("/one") void go() {}
  void go(int x) {}
  @RequestMapping("/in") static class In { @PutMapping("/p") void go() {} }
}"#);
    let handlers: Vec<_> = m
        .entities()
        .filter(|e| e.props().contains_key(PROP_ENDPOINTS))
        .map(|e| (e.key().to_string(), strings(e.props().get(PROP_ENDPOINTS))))
        .collect();
    assert_eq!(handlers.len(), 2, "{handlers:?}");
    assert_eq!(eps(&m, "In", "go"), ["PUT /in/p"]);
    // `go(int)` has no mapping.
    let plain = m
        .entities()
        .filter(|e| e.name() == "go" && e.parent().is_some_and(|p| p.name() == "A"))
        .filter(|e| !e.props().contains_key(PROP_ENDPOINTS))
        .count();
    assert_eq!(plain, 1);
}

const API: &str = r#"package p;
@RequestMapping("/base")
interface Api {
  @GetMapping("/x/{id}") void get(String id);
  @PostMapping("/y") void post();
}"#;

#[test]
fn interface_endpoints_inherited_with_prefix() {
    let (m, _) = build(
        &[
            ("src/main/java/p/Api.java", API),
            (
                "src/main/java/p/Ctl.java",
                r#"package p; class Ctl implements Api { @Override public void get(String id) {}
                   @PostMapping("/own") public void post() {} }"#,
            ),
            (
                "src/main/java/p/Sub.java",
                r#"package p; @RequestMapping("/sub") class Sub implements Api { public void get(String id) {} }"#,
            ),
        ],
        &[],
    );
    assert_eq!(eps(&m, "Ctl", "get"), ["GET /base/x/{id}"]);
    // Own mapping wins; the interface method keeps its endpoint.
    assert_eq!(eps(&m, "Ctl", "post"), ["POST /base/own"]);
    assert_eq!(eps(&m, "Api", "post"), ["POST /base/y"]);
    // Spring: the class-level mapping of the implementation replaces the interface's.
    assert_eq!(eps(&m, "Sub", "get"), ["GET /sub/x/{id}"]);
}

#[test]
fn generated_interface_is_used_but_gets_no_entity() {
    let gen_api = API
        .replace("package p;", "package gen;")
        .replace("Api", "HabApi");
    let gen_path = "target/generated-sources/openapi/src/main/java/gen/HabApi.java";
    let (m, stats) = build(
        &[
            (
                "src/main/java/p/Ctl.java",
                "package p; import gen.HabApi; class Ctl implements HabApi { public void get(String id) {} }",
            ),
            (
                "src/main/java/p/Ctl2.java",
                "package p; class Ctl2 implements OtherApi { public void o() {} }",
            ),
            (
                "src/main/java/p/Lost.java",
                "package p; class Lost implements MissingApi {}",
            ),
        ],
        &[
            (gen_path, &gen_api),
            (
                "build/generated/x/g/OtherApi.java",
                r#"package g; interface OtherApi { @GetMapping("/o") void o(); }"#,
            ),
        ],
    );
    assert_eq!(eps(&m, "Ctl", "get"), ["GET /base/x/{id}"]);
    assert_eq!(eps(&m, "Ctl2", "o"), ["GET /o"]);
    assert_eq!(stats.unresolved_api, 1, "{stats:?}");
    assert_eq!(stats.generated_files, 2);
}

#[test]
fn feign_interface_and_fallback_serve_nothing() {
    let m = build(
        &[
            (
                "p/FooClient.java",
                r#"package p; @FeignClient(name = "x") @RequestMapping("/f")
                interface FooClient { @GetMapping("/a") void a(); }"#,
            ),
            (
                "p/FooClientFallback.java",
                "package p; class FooClientFallback implements FooClient { public void a() {} }",
            ),
        ],
        &[],
    )
    .0;
    assert!(eps(&m, "FooClient", "a").is_empty());
    assert!(eps(&m, "FooClientFallback", "a").is_empty());
}

#[test]
fn transitive_and_abstract_superclass_inheritance() {
    let m = build(
        &[
            (
                "p/A.java",
                r#"package p; @RequestMapping("/a") interface A { @GetMapping("/m") void m(); }"#,
            ),
            ("p/B.java", "package p; abstract class B implements A { }"),
            (
                "p/C.java",
                "package p; class C extends B { public void m() {} }",
            ),
            (
                "p/Base.java",
                r#"package p; abstract class Base { @PutMapping("/z") abstract void z(); }"#,
            ),
            (
                "p/D.java",
                r#"package p; @RequestMapping("/d") class D extends Base { void z() {} }"#,
            ),
        ],
        &[],
    )
    .0;
    assert_eq!(eps(&m, "C", "m"), ["GET /a/m"]);
    assert_eq!(eps(&m, "D", "z"), ["PUT /d/z"]);
}

#[test]
fn same_simple_name_generated_apis_resolve_by_import() {
    let m = build(
        &[(
            "p/Ctl.java",
            "package p; import two.HabApi; class Ctl implements HabApi { public void h() {} }",
        )],
        &[
            (
                "target/generated-sources/o/one/HabApi.java",
                r#"package one; interface HabApi { @GetMapping("/one") void h(); }"#,
            ),
            (
                "target/generated-sources/o/two/HabApi.java",
                r#"package two; interface HabApi { @GetMapping("/two") void h(); }"#,
            ),
        ],
    )
    .0;
    assert_eq!(eps(&m, "Ctl", "h"), ["GET /two"]);
}

#[test]
fn test_sources_are_not_read() {
    let (m, stats) = build(
        &[(
            "src/test/java/A.java",
            r#"class A { @GetMapping("/t") void a() {} }"#,
        )],
        &[],
    );
    assert_eq!(stats.files, 0);
    assert!(eps(&m, "A", "a").is_empty());
}

// ---- GWT RPC ----

const REMOTE: &[(&str, &str)] = &[
    (
        "src/main/java/c/remote/ISvc.java",
        "package c.remote;
import com.google.gwt.user.client.rpc.RemoteService;
public interface ISvc extends RemoteService {
  String go(int a);
  class Util { public static ISvcAsync getInstance() { return GWT.create(ISvc.class); } }
}",
    ),
    (
        "src/main/java/c/remote/ISvcAsync.java",
        "package c.remote;
public interface ISvcAsync { void go(int a, AsyncCallback<String> cb); }",
    ),
    (
        "src/main/java/c/remote/ISub.java",
        "package c.remote; public interface ISub extends ISvc { void more(); }",
    ),
    (
        "src/main/java/c/remote/ISubAsync.java",
        "package c.remote; public interface ISubAsync extends ISvcAsync { void more(AsyncCallback<Void> cb); }",
    ),
    (
        "src/main/java/c/other/IFoo.java",
        "package c.other; public interface IFoo { void run(); }",
    ),
    (
        "src/main/java/c/other/IFooAsync.java",
        "package c.other; public interface IFooAsync { void run(Cb cb); }",
    ),
    (
        "src/main/java/s/SvcImpl.java",
        "package s; import c.remote.ISvc;
public class SvcImpl extends RemoteServiceServlet implements ISvc { public String go(int a) { return null; } }",
    ),
    (
        "src/main/java/s/SubImpl.java",
        "package s; import c.remote.ISub;
public class SubImpl extends s.SvcImpl implements ISub { public void more() {} }",
    ),
    (
        "src/main/java/c/ui/Page.java",
        "package c.ui;
import c.remote.*;
import c.other.*;
public class Page {
  private ISvcAsync field = ISvc.Util.getInstance();
  private IFooAsync foo;
  void viaUtil() { ISvc.Util.getInstance().go(1, cb); }
  void viaField() { field.go(1, cb); }
  void viaThis() { this.field.go(2, cb); }
  void viaLocal() { ISvcAsync s = ISvc.Util.getInstance(); s.go(1, cb); }
  void viaVar() { var s = ISvc.Util.getInstance(); s.go(1, cb); }
  void viaCreate() { ISvcAsync s = GWT.create(ISvc.class); s.go(1, cb); }
  void viaCast() { ((ISvcAsync) GWT.create(ISvc.class)).go(1, cb); }
  void viaParam(ISvcAsync p) { p.go(1, cb); }
  void viaSub(ISubAsync p) { p.more(cb); }
  void shadowed() { IFooAsync field = foo; field.run(cb); }
  void notRpc() { foo.run(cb); field.toString(); }
  void inLambda() { run(() -> field.go(1, cb)); }
  Page() { field.go(0, cb); }
  void scoped1() { { IFooAsync field = foo; field.run(cb); } field.go(1, cb); }
  void scoped2() { run(() -> { IFooAsync field = foo; field.run(cb); }); field.go(1, cb); }
  void scoped3() { new Thread() { void r() { IFooAsync field = foo; } }; field.go(1, cb); }
  void scoped4() { for (IFooAsync field : foos) { field.run(cb); } field.go(1, cb); }
  void scoped5() { IFooAsync field = foo; { field.run(cb); } field.go(1, cb); }
}",
    ),
    (
        "src/main/java/c/ui/External.java",
        "package c.ui;
public class External {
  void a() { com.x.IGone.Util.getInstance().f(cb); }
  void b() { IGone.Util.getInstance().f(cb); }
  void c() { ((IGoneAsync) GWT.create(IGone.class)).f(cb); }
  void d() { IGoneAsync typedOnly = null; typedOnly.f(cb); }
  void e() { Helper.Cache.get().f(cb); }
  void f() { ((Bundle) GWT.create(Bundle.class)).img(); }
  void i() { IGone.Util.getInstance().f(1, // comment
    cb); }
  void h() { Bundle.Util.get().img(); }
  void g() { GWT.create(Other.class).f(cb); }
}",
    ),
    (
        "src/main/java/g/Gone.java",
        "package g; public class Gone extends RemoteServiceServlet implements IGone { public void f(int a) {} }",
    ),
];

fn rpc_model() -> (Model, JavaStats) {
    build(REMOTE, &[])
}

/// Names (`Type.method`) of the targets the method `caller` of `Page` reaches.
fn targets(m: &Model, ty: &str, caller: &str) -> Vec<String> {
    let rel = rpc_relation();
    let mut v: Vec<String> = m
        .entities()
        .filter(|e| e.name() == caller && e.parent().is_some_and(|p| p.name() == ty))
        .flat_map(|e| e.outgoing().collect::<Vec<_>>())
        .filter(|r| r.kind() == &rel)
        .map(|r| {
            assert_eq!(r.provenance(), Provenance::Frontend);
            assert!(!r.sites().is_empty());
            format!(
                "{}.{}",
                r.to().parent().map_or("", |p| p.name()),
                r.to().name()
            )
        })
        .collect();
    v.sort();
    v
}

fn calls(m: &Model, ty: &str, caller: &str) -> Vec<String> {
    m.entities()
        .filter(|e| e.name() == caller && e.parent().is_some_and(|p| p.name() == ty))
        .flat_map(|e| strings(e.props().get(PROP_RPC_CALLS)))
        .collect()
}

#[test]
fn rpc_call_shapes_reach_the_implementation() {
    let (m, stats) = rpc_model();
    for c in [
        "viaUtil",
        "viaField",
        "viaThis",
        "viaLocal",
        "viaVar",
        "viaCreate",
        "viaCast",
        "viaParam",
        "inLambda",
        "scoped1",
        "scoped2",
        "scoped3",
        "scoped4",
    ] {
        assert_eq!(targets(&m, "Page", c), ["SvcImpl.go"], "{c}");
        assert!(
            calls(&m, "Page", c)[0].starts_with("c.remote.ISvc#go/"),
            "{c}: {:?}",
            calls(&m, "Page", c)
        );
    }
    // The arity is the call's, callback included.
    assert_eq!(calls(&m, "Page", "viaThis"), ["c.remote.ISvc#go/2"]);
    // The constructor is a caller too; `Page` is not an `ISvc` method (no name clash).
    let ctor = m
        .entities()
        .find(|e| e.name() == "Page" && e.kind() == model_core::EntityKind::Constructor)
        .unwrap();
    assert_eq!(
        strings(ctor.props().get(PROP_RPC_CALLS)),
        ["c.remote.ISvc#go/2"]
    );
    // A sub-service is reached through its implementation, which inherits `go`.
    assert_eq!(targets(&m, "Page", "viaSub"), ["SubImpl.more"]);
    // Shadowed by a local of another type, or not a service at all.
    assert!(calls(&m, "Page", "shadowed").is_empty());
    assert!(calls(&m, "Page", "scoped5").is_empty());
    assert!(calls(&m, "Page", "notRpc").is_empty());
    assert!(stats.rpc_linked > 0);
}

#[test]
fn services_are_marked_and_twins_are_not_callers() {
    let (m, _) = rpc_model();
    let flag = |name: &str| {
        m.entities()
            .find(|e| e.name() == name && e.kind().is_type())
            .unwrap()
            .props()
            .get(PROP_RPC_SERVICE)
            .cloned()
    };
    assert_eq!(flag("ISvc"), Some(Value::Bool(true)));
    assert_eq!(flag("ISub"), Some(Value::Bool(true)));
    // Same `Async` naming, no `RemoteService`.
    assert_eq!(flag("IFoo"), None);
    // The service interface and its `Util` call nothing.
    assert!(
        m.entities()
            .filter(|e| e.file().is_some_and(|f| f.contains("/c/remote/")))
            .all(|e| !e.props().contains_key(PROP_RPC_CALLS))
    );
}

#[test]
fn unresolved_service_is_linked_by_the_implementing_names() {
    let (m, stats) = rpc_model();
    assert!(calls(&m, "External", "a").is_empty());
    // `IGone` is not in the project, nor `IGoneAsync`: `Gone implements IGone`
    // is the only hint, and `X.Util.getInstance()` alone is not confirmed.
    assert!(calls(&m, "External", "b").is_empty());
    assert!(calls(&m, "External", "c").is_empty());
    assert!(stats.rpc_maybe >= 2, "{stats:?}");
    // Typed `IGoneAsync`: the service is `IGone`, found by name.
    assert_eq!(calls(&m, "External", "d"), ["IGone#f/1"]);
    assert_eq!(targets(&m, "External", "d"), ["Gone.f"]);
    // Not services.
    for c in ["e", "f", "g", "h"] {
        assert!(calls(&m, "External", c).is_empty(), "{c}");
    }
}

#[test]
fn rpc_does_not_break_the_moose_export() {
    let (m, _) = rpc_model();
    let mut out = Vec::new();
    let opts = model_moose::ExportOptions {
        root: None,
        no_sources: true,
    };
    let report = model_moose::export_json(&m, &mut out, &opts).unwrap();
    assert!(report.entity_count() > 0);
    assert!(report.entities.contains_key("Method"));
    // Extension relation: reported, not written.
    assert!(
        report
            .skipped_relations
            .keys()
            .any(|k| k.starts_with("gwt-rpc")),
        "{:?}",
        report.skipped_relations
    );
    // Props are extensions too: no endpoint or RPC fact leaks into the JSON.
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("http.endpoints") && !text.contains("rpc.calls"));
}

#[test]
fn http_endpoint_entities_stay_out_of_the_moose_export() {
    let m = one(r#"@RequestMapping("/a")
class A { @GetMapping("/x") void h() {} @PostMapping("/x") void i() {} }"#);
    let mut out = Vec::new();
    let opts = model_moose::ExportOptions {
        root: None,
        no_sources: true,
    };
    let report = model_moose::export_json(&m, &mut out, &opts).unwrap();
    // Reported as not exported, never written as a Famix class.
    assert_eq!(report.skipped_entities.get(crate::HTTP_ENDPOINT), Some(&2));
    assert!(
        report
            .skipped_relations
            .keys()
            .any(|k| k.starts_with("http.endpoints")),
        "{:?}",
        report.skipped_relations
    );
    // The handler methods are still exported.
    assert_eq!(report.entities.get("Method"), Some(&2));
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("GET /a/x") && !text.contains("http.verb"));
}

#[test]
fn enrichment_is_deterministic() {
    let (a, _) = rpc_model();
    let (b, _) = rpc_model();
    let dump = |m: &Model| {
        m.relations()
            .filter(|r| r.kind() == &rpc_relation())
            .map(|r| format!("{}->{} {:?}", r.from().key(), r.to().key(), r.sites()))
            .collect::<Vec<_>>()
    };
    assert_eq!(dump(&a), dump(&b));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("model_lang_java_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn write(dir: &std::path::Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

/// `enrich` on a directory: build-generated sources found on disk, tests skipped.
#[test]
fn enrich_reads_a_project_directory() {
    let d = scratch("dir");
    write(
        &d,
        "src/main/java/p/Ctl.java",
        "package p; import gen.HabApi; class Ctl implements HabApi { public void get(String id) {} }",
    );
    write(
        &d,
        "src/test/java/p/T.java",
        r#"package p; class T { @GetMapping("/t") void t() {} }"#,
    );
    write(
        &d,
        "target/generated-sources/openapi/src/main/java/gen/HabApi.java",
        &API.replace("package p;", "package gen;")
            .replace("Api", "HabApi"),
    );
    let mut b = ModelBuilder::new();
    model_treesitter::extract(&mut b, &d, &ExtractOptions::default()).unwrap();
    let stats = crate::enrich(&mut b, &d).unwrap();
    let m = b.build();
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(eps(&m, "Ctl", "get"), ["GET /base/x/{id}"]);
    assert_eq!((stats.files, stats.generated_files), (1, 1));
    assert_eq!(stats.handlers, 1);
}

/// Generated twins are never main sources, whatever the ignore rules say
/// (CI has no global gitignore hiding `target/`).
#[test]
fn generated_trees_are_not_main_sources() {
    let d = scratch("gen");
    write(&d, "src/main/java/p/A.java", "package p; class A {}");
    write(
        &d,
        "target/generated-sources/x/p/B.java",
        "package p; class B {}",
    );
    write(&d, "build/generated/y/p/C.java", "package p; class C {}");
    let mut b = ModelBuilder::new();
    let opts = ExtractOptions::default();
    model_treesitter::extract(&mut b, &d, &opts).unwrap();
    let stats = crate::enrich(&mut b, &d).unwrap();
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!((stats.files, stats.generated_files), (1, 2));
}

/// Real project check, skipped without `MODEL_FIXTURES` (Structure's `fixtures/`).
#[test]
fn realworld_controllers_have_endpoints() {
    let Ok(base) = std::env::var("MODEL_FIXTURES") else {
        return;
    };
    let root = std::path::Path::new(&base).join("spring-boot-realworld-example-app");
    if !root.exists() {
        return;
    }
    let mut b = ModelBuilder::new();
    model_treesitter::extract(&mut b, &root, &ExtractOptions::default()).unwrap();
    let stats = crate::enrich(&mut b, &root).unwrap();
    let m = b.build();
    assert_eq!(stats.endpoints, 19, "{stats:?}");
    assert_eq!(eps(&m, "ArticleApi", "article"), ["GET /articles/{slug}"]);
    assert_eq!(eps(&m, "UsersApi", "userLogin"), ["POST /users/login"]);
    assert_eq!(eps(&m, "CurrentUserApi", "updateProfile"), ["PUT /user"]);
}
