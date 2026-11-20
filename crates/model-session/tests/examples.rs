//! `examples/sessions.rs` on `docs/examples/shop`: the layers and counts
//! quoted in `docs/examples.md` (scenarios a and c).

use std::process::Command;

/// Runs the example through cargo (already built by `cargo test`, so this is
/// quick; it also works when only this test target was selected).
fn example(name: &str) -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.args(["run", "-q", "-p", "model-session", "--example", name]);
    c
}

#[test]
fn sessions_run_only_the_layers_needed_on_the_sample() {
    let out = example("sessions").output().expect("run sessions");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    for line in [
        "all, Named+WithMembers: 71 entities, ran [Structure], 8 files, 0 indexing events, 0 indexers started",
        "Typed kinds: constructor, field, local_variable, method, parameter",
        "again, Named: 71 entities, ran [], 0 files, 0 indexing events, 0 indexers started",
        "package shop/model, WithMembers: 31 entities, ran [Structure], 3 files, 0 indexing events, 0 indexers started",
        "all, Invocation: 80 entities, ran [Structure, Resolution], 8 files, 0 indexing events, 0 indexers started",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
}
