use model_core::{EntityKind, Model, ModelBuilder, Provenance, RelationKind, Value};
use model_treesitter::{ExtractOptions, extract_sources};

use crate::{HTTP_ENDPOINT, endpoints, enrich_sources};

fn model(files: &[(&str, &str)]) -> (Model, crate::PythonStats) {
    let mut b = ModelBuilder::new();
    extract_sources(&mut b, files, &ExtractOptions::default()).unwrap();
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .collect();
    let st = enrich_sources(&mut b, &owned);
    (b.build(), st)
}

fn prop(m: &Model, key: &str, name: &str) -> Option<String> {
    match m.by_key(key)?.props().get(name)? {
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

const FLASK: &str = r#"from flask import Flask, Blueprint
app = Flask(__name__)
bp = Blueprint("bp", __name__, url_prefix="/api")

@app.route("/")
def index():
    pass

@app.route("/items", methods=["GET", "post"])
def items():
    pass

@bp.get("/users/<int:id>")
def user(id):
    pass

@other.get("/skipped")
def skipped():
    pass

@app.route(f"/{x}")
def dynamic():
    pass
"#;

#[test]
fn flask_routes() {
    let eps = endpoints(FLASK);
    let got: Vec<_> = eps
        .iter()
        .map(|e| (e.framework.as_str(), e.method.as_str(), e.path.as_str()))
        .collect();
    // `other.get` is a Flask verb on an unknown receiver: the file imports
    // flask, so it counts; the f-string route has no static path.
    assert_eq!(
        got,
        [
            ("flask", "GET", "/"),
            ("flask", "GET", "/items"),
            ("flask", "POST", "/items"),
            ("flask", "GET", "/api/users/<int:id>"),
            ("flask", "GET", "/skipped"),
        ]
    );
}

#[test]
fn endpoints_hang_from_their_handler() {
    let (m, st) = model(&[("app/main.py", FLASK)]);
    assert_eq!(st.endpoints, 5);
    let e = m.entities().find(|e| e.name() == "GET /items").unwrap();
    assert_eq!(e.kind_name(), HTTP_ENDPOINT);
    assert_eq!(e.kind(), EntityKind::Variable);
    let parent = e.parent().unwrap();
    assert_eq!(parent.name(), "items");
    assert!(parent.key().ends_with("items()."), "{}", parent.key());
    let k = e.key().to_string();
    assert_eq!(prop(&m, &k, "http.verb").as_deref(), Some("GET"));
    assert_eq!(prop(&m, &k, "http.path").as_deref(), Some("/items"));
    assert_eq!(prop(&m, &k, "http.framework").as_deref(), Some("flask"));
    assert_eq!(prop(&m, &k, "http.handler").as_deref(), Some("items"));
    // The kind has no FM3 class.
    assert!(e.kind_info().is_some_and(|k| k.fm3.is_none()));
}

#[test]
fn fastapi_routers_with_prefix() {
    let src = r#"from fastapi import APIRouter, FastAPI
router = APIRouter(prefix="/v1")
app = FastAPI()

@router.get("/users")
async def users():
    pass

@router.post("/users")
def create():
    pass

@app.api_route("/multi", methods=["PUT", "PATCH"])
def multi():
    pass

@router.get(path="/kw")
def kw():
    pass
"#;
    let got: Vec<_> = endpoints(src)
        .iter()
        .map(|e| (e.framework.as_str(), e.method.clone(), e.path.clone()))
        .collect();
    assert_eq!(
        got,
        [
            ("fastapi", "GET".into(), "/v1/users".into()),
            ("fastapi", "POST".into(), "/v1/users".into()),
            ("fastapi", "PUT".into(), "/multi".into()),
            ("fastapi", "PATCH".into(), "/multi".into()),
            ("fastapi", "GET".into(), "/v1/kw".into()),
        ]
    );
}

#[test]
fn verbs_need_a_framework() {
    let src = "@client.get('/x')\ndef f(): pass\n";
    assert!(endpoints(src).is_empty());
    assert!(endpoints("@pytest.fixture\ndef f(): pass\n").is_empty());
}

#[test]
fn django_urlpatterns() {
    let urls = r#"from django.urls import path, re_path, include
from . import views
from .views import UserView

urlpatterns = [
    path("users/", views.list_users, name="users"),
    path("users/<int:pk>/", UserView.as_view()),
    re_path(r"^legacy/$", views.legacy),
    path("api/", include("api.urls")),
    path("lambda/", lambda r: None),
]
"#;
    let views = "def list_users(request):\n    pass\n\nclass UserView:\n    pass\n\ndef legacy(r):\n    pass\n";
    let (m, st) = model(&[("shop/urls.py", urls), ("shop/views.py", views)]);
    assert_eq!(st.endpoints, 4);
    assert_eq!(st.resolved_views, 3);
    let any: Vec<_> = m
        .entities()
        .filter(|e| e.kind_name() == HTTP_ENDPOINT)
        .map(|e| e.name().to_string())
        .collect();
    assert_eq!(any.len(), 4, "{any:?}");
    assert!(any.contains(&"ANY users/".to_string()), "{any:?}");
    let e = m.entities().find(|e| e.name() == "ANY users/").unwrap();
    assert_eq!(e.parent().unwrap().name(), "urls.py");
    let k = e.key().to_string();
    assert_eq!(prop(&m, &k, "http.route_name").as_deref(), Some("users"));
    assert_eq!(prop(&m, &k, "http.framework").as_deref(), Some("django"));
    let calls: Vec<_> = m
        .relations()
        .filter(|r| r.kind() == &RelationKind::Calls && r.from().kind_name() == HTTP_ENDPOINT)
        .map(|r| (r.from().name().to_string(), r.to().name().to_string()))
        .collect();
    assert!(
        calls.contains(&("ANY users/<int:pk>/".into(), "UserView".into())),
        "{calls:?}"
    );
    assert!(
        calls.contains(&("ANY users/".into(), "list_users".into())),
        "{calls:?}"
    );
    let prov = m
        .relations()
        .find(|r| r.from().kind_name() == HTTP_ENDPOINT)
        .map(|r| r.provenance());
    assert_eq!(prov, Some(Provenance::Frontend));
}

#[test]
fn a_file_without_routes_adds_nothing() {
    let (m, st) = model(&[("a.py", "def f():\n    return 1\n")]);
    assert_eq!(st.endpoints, 0);
    assert!(m.entities().all(|e| e.kind_name() != HTTP_ENDPOINT));
}

#[test]
fn same_route_twice_gets_two_entities() {
    let src = "from flask import Flask\napp = Flask(__name__)\n@app.route('/a')\n@app.route('/a')\ndef f(): pass\n";
    let (m, st) = model(&[("a.py", src)]);
    assert_eq!(st.endpoints, 2);
    assert_eq!(
        m.entities()
            .filter(|e| e.kind_name() == HTTP_ENDPOINT)
            .count(),
        2
    );
}
