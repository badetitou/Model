use model_core::{Model, ModelBuilder, Provenance, RelationKind, Value};
use model_treesitter::ExtractOptions;

use crate::{TypeScriptStats, enrich_sources};

fn model(sources: &[(&str, &str)]) -> (Model, TypeScriptStats) {
    let mut b = ModelBuilder::new();
    model_treesitter::extract_sources(&mut b, sources, &ExtractOptions::default()).unwrap();
    let mut warnings = Vec::new();
    let st = enrich_sources(&mut b, sources, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    (b.build(), st)
}

/// `(kind, label, owner name)` of the HTTP entities of `kind`, sorted.
fn http(m: &Model, kind: &str) -> Vec<(String, String)> {
    let mut v: Vec<_> = m
        .entities()
        .filter(|e| e.kind_name() == kind)
        .map(|e| {
            (
                e.name().to_string(),
                e.parent().map(|p| p.name().to_string()).unwrap_or_default(),
            )
        })
        .collect();
    v.sort();
    v
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

const ANGULAR: &str = r#"
import { HttpClient } from '@angular/common/http';
@Injectable({ providedIn: 'root' })
export class UserService {
  private direct = inject(HttpClient);
  constructor(private http: HttpClient, private api: ApiClient) {}
  list() { return this.http.get<User[]>('/api/users?limit=3'); }
  load(id: number) { return this.http.get(`${this.base}/users/${id}`); }
  save(u: User) { return this.http.post('http://host:8080/api/users/', u); }
  drop() { return this.direct.delete('/api/users/1'); }
  other() { return this.api.get('/not/http'); }
  custom() { return this.http.request('PUT', '/api/x'); }
}
"#;

#[test]
fn angular_http_client_calls() {
    let (m, st) = model(&[("src/user.service.ts", ANGULAR)]);
    assert_eq!(st.calls, 5);
    assert_eq!(st.endpoints, 0);
    assert_eq!(
        http(&m, "http.call"),
        [
            pair("DELETE /api/users/1", "drop"),
            pair("GET /api/users", "list"),
            pair("GET /{base}/users/{id}", "load"),
            pair("POST /api/users", "save"),
            pair("PUT /api/x", "custom"),
        ]
    );
    let list = m
        .entities()
        .find(|e| e.name() == "list" && e.kind_name() == "method")
        .unwrap();
    assert_eq!(
        list.props().get("http.calls"),
        Some(&Value::from(vec!["GET /api/users"]))
    );
    let call = list.children().next().unwrap();
    assert_eq!(
        call.key(),
        format!("{}call-L7C18:", list.key()),
        "key hangs under the owner"
    );
    assert_eq!(call.language().map(|l| l.as_str()), Some("typescript"));
    assert_eq!(call.file(), Some("src/user.service.ts"));
    assert_eq!(
        call.props().get("http.library"),
        Some(&Value::from("angular"))
    );
}

#[test]
fn relations_are_frontend_with_the_call_site() {
    let (m, _) = model(&[("a.ts", ANGULAR)]);
    let rels: Vec<_> = m
        .relations()
        .filter(|r| r.kind() == &RelationKind::Other("http.calls".into()))
        .collect();
    assert_eq!(rels.len(), 5);
    for r in rels {
        assert_eq!(r.provenance(), Provenance::Frontend);
        assert_eq!(r.sites().len(), 1);
        assert_eq!(r.to().parent().map(|p| p.id()), Some(r.from().id()));
    }
}

#[test]
fn fetch_and_axios_forms() {
    let src = r#"
export function a() {
  fetch('/a');
  fetch('/b', { method: 'POST' });
  fetch('/c', opts);
  fetch(`/d/${id}`, { ...o });
}
export const b = () => {
  axios.get('/e');
  axios({ url: '/f', method: 'delete' });
  axios('/g');
  axios('/h', { method: 'PATCH' });
  const base = '/i';
  return axios.put(base + '/x', {});
};
class C { run() { const u = '/j'; return fetch(u, {}); } }
"#;
    let (m, st) = model(&[("src/a.ts", src)]);
    assert_eq!(st.calls, 10);
    assert_eq!(
        http(&m, "http.call"),
        [
            pair("ANY /c", "a"),
            pair("ANY /d/{id}", "a"),
            pair("DELETE /f", "b"),
            pair("GET /a", "a"),
            pair("GET /e", "b"),
            pair("GET /g", "b"),
            pair("GET ?", "run"),
            pair("PATCH /h", "b"),
            pair("POST /b", "a"),
            pair("PUT /i/x", "b"),
        ]
    );
    // `b` is a ts.arrow_function: it owns its calls.
    let b = m.entities().find(|e| e.name() == "b").unwrap();
    assert_eq!(b.kind_name(), "ts.arrow_function");
}

#[test]
fn top_level_calls_belong_to_the_file() {
    let (m, _) = model(&[("src/boot.ts", "fetch('/boot');\n")]);
    let owner = m
        .entities()
        .find(|e| e.kind_name() == "http.call")
        .and_then(|e| e.parent())
        .unwrap();
    assert_eq!(owner.kind_name(), "ts.module");
}

#[test]
fn tsx_files_are_read() {
    let src = "export function App() { const go = () => fetch('/x'); return <b onClick={go}/>; }\n";
    let (m, st) = model(&[("src/App.tsx", src)]);
    assert_eq!(st.calls, 1);
    assert_eq!(http(&m, "http.call")[0].0, "GET /x");
}

const NEST: &str = r#"
@Controller('users')
export class UsersController {
  @Get()
  list() {}

  @Get(':id')
  one(@Param('id') id: string) {}

  @Post()
  @HttpCode(201)
  create() {}

  @Delete(':id/roles/:role(\\d+)')
  drop() {}

  @Get(PATH)
  odd() {}

  @Get('/legacy/' + SUFFIX)
  concat() {}

  @Cacheable()
  helper() {}
}

@Controller({ path: 'v2/items' })
class Items {
  @All('x') any() {}
}

class NotAController {
  @Get('y') nope() {}
}
"#;

#[test]
fn nest_controllers() {
    let (m, st) = model(&[("src/users.controller.ts", NEST)]);
    assert_eq!(st.endpoints, 7);
    assert_eq!(
        http(&m, "http.endpoint"),
        [
            pair("ANY /v2/items/x", "any"),
            pair("DELETE /users/{id}/roles/{role}", "drop"),
            pair("GET /users", "list"),
            pair("GET /users/legacy/{SUFFIX}", "concat"),
            pair("GET /users/{id}", "one"),
            pair("GET ?", "odd"),
            pair("POST /users", "create"),
        ]
    );
    let one = m
        .entities()
        .find(|e| e.name() == "one" && e.kind_name() == "method")
        .unwrap();
    assert_eq!(
        one.props().get("http.endpoints"),
        Some(&Value::from(vec!["GET /users/{id}"]))
    );
    let e = one
        .children()
        .find(|c| c.kind_name() == "http.endpoint")
        .unwrap();
    assert_eq!(
        e.props().get("http.framework"),
        Some(&Value::from("nestjs"))
    );
    assert_eq!(e.props().get("http.verb"), Some(&Value::from("GET")));
}

#[test]
fn express_style_routes() {
    let src = r#"
const app = express();
const users = Router();
users.get('/users/:id', auth, (req, res) => {});
app.post('/users', create);
app.use('/api', users);
app.get('setting');
app.all('/any', h);
cache.get('/not-a-route', h);
axios.get('/client', cfg);
export function mount(server: Server) {
  server.delete('/things/:thing', (req, res) => {});
}
"#;
    let (m, st) = model(&[("src/server.ts", src)]);
    assert_eq!(st.endpoints, 4);
    assert_eq!(st.calls, 1);
    assert_eq!(
        http(&m, "http.endpoint"),
        [
            pair("ANY /any", "server.ts"),
            pair("DELETE /things/{thing}", "mount"),
            pair("GET /users/{id}", "server.ts"),
            pair("POST /users", "server.ts"),
        ]
    );
    assert_eq!(http(&m, "http.call"), [pair("GET /client", "server.ts")]);
}

#[test]
fn enrichment_is_idempotent() {
    let sources = [("src/a.ts", ANGULAR), ("src/c.ts", NEST)];
    let mut b = ModelBuilder::new();
    model_treesitter::extract_sources(&mut b, &sources, &ExtractOptions::default()).unwrap();
    let mut w = Vec::new();
    let first = enrich_sources(&mut b, &sources, &mut w);
    let n = b.entity_count();
    let second = enrich_sources(&mut b, &sources, &mut w);
    assert_eq!(b.entity_count(), n);
    assert_eq!(second.calls, 0);
    assert_eq!(second.endpoints, 0);
    assert!(first.calls > 0 && first.endpoints > 0);
    let m = b.build();
    assert_eq!(
        m.relations()
            .filter(|r| matches!(r.kind(), RelationKind::Other(_)))
            .count(),
        first.calls + first.endpoints
    );
}

#[test]
fn json_round_trip_keeps_the_http_entities() {
    let (m, _) = model(&[("src/a.ts", ANGULAR)]);
    let json = serde_json::to_string(&m).unwrap();
    let back: Model = serde_json::from_str(&json).unwrap();
    assert_eq!(http(&back, "http.call"), http(&m, "http.call"));
}

#[test]
fn kinds_are_data_with_graphql_concepts() {
    assert_eq!(crate::kinds().len(), 2, "kinds.json did not parse");
    let mut b = ModelBuilder::new();
    crate::declare_kinds(&mut b);
    let m = b.build();
    for k in ["http.call", "http.endpoint"] {
        let id = m.metamodel().find(k).unwrap();
        let info = m.metamodel().get(id).unwrap();
        assert!(info.graphql_type.is_some() && info.graphql_root.is_some());
        // Not a Famix concept: no FM3 class is invented for it.
        assert_eq!(info.fm3, None);
    }
}

/// Every `Famix-TypeScript-Entities.*` class named by a kind (ours and the
/// tree-sitter ones) exists in FamixTypeScript.
#[test]
fn fm3_names_resolve_against_famixtypescript() {
    let classes: std::collections::HashSet<&str> =
        include_str!("../tests/fixtures/famixtypescript-classes.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect();
    assert!(classes.contains("Class") && classes.contains("ArrowFunction"));
    let mut b = ModelBuilder::new();
    model_treesitter::declare_kinds(&mut b);
    crate::declare_kinds(&mut b);
    let m = b.build();
    let mut checked = 0;
    for (_, k) in m.metamodel().iter() {
        let Some(fm3) = &k.fm3 else { continue };
        let Some(name) = fm3.strip_prefix("Famix-TypeScript-Entities.") else {
            continue;
        };
        assert!(
            classes.contains(name),
            "{}: unknown FM3 class {fm3}",
            k.name
        );
        checked += 1;
    }
    assert!(checked >= 6, "{checked}");
}
