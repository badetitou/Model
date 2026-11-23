use serde_json::Value;

use crate::Engine;

fn run(engine: &Engine, query: &str) -> Value {
    let request = serde_json::json!({ "query": query }).to_string();
    serde_json::from_str(&engine.execute(&request).unwrap()).unwrap()
}

fn engine() -> Engine {
    let mut e = Engine::new();
    e.add_file(
        "src/Greeter.java",
        "package demo;\npublic class Greeter {\n  public String hello() { return helper(); }\n  private String helper() { return \"hi\"; }\n}\n",
    );
    e.add_file(
        "src/app.ts",
        "export class App { run(): void { this.stop(); } stop(): void {} }\n",
    );
    e.add_file("README.md", "# ignored");
    e.build().unwrap();
    e
}

#[test]
fn indexes_in_memory_sources_and_answers_graphql() {
    let e = engine();
    let json = run(&e, "{ classes { nodes { name } } }");
    assert!(json["errors"].is_null(), "{json}");
    let names: Vec<&str> = json["data"]["classes"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Greeter"), "{names:?}");
    assert!(names.contains(&"App"), "{names:?}");
}

#[test]
fn reports_a_query_error_in_the_response() {
    let json = run(&engine(), "{ nope }");
    assert!(json["errors"].is_array());
}

#[test]
fn refuses_before_build_and_on_bad_json() {
    assert!(Engine::new().execute("{}").is_err());
    assert!(engine().execute("nope").is_err());
}
