//! Tree-sitter + SCIP on real projects, as Structure's `link_fixture` test.
//! Fixtures come from `MODEL_FIXTURES` (default `<workspace>/fixtures`) and
//! are skipped when absent, e.g.
//! `MODEL_FIXTURES=../Structure/fixtures cargo test -p model-treesitter --test fixtures`.

use std::path::PathBuf;

use model_core::{EntityKind, EntityRef, Model, ModelBuilder};
use model_link::{LinkOptions, LinkStats};
use model_treesitter::ExtractOptions;

fn base() -> PathBuf {
    std::env::var("MODEL_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"))
}

fn build(name: &str) -> Option<(Model, LinkStats)> {
    let root = base().join(name);
    let index = root.join("index.scip");
    if !index.is_file() {
        eprintln!("skipping {name}: {} not found", index.display());
        return None;
    }
    let mut b = ModelBuilder::new();
    let opts = ExtractOptions {
        relations: false,
        ..ExtractOptions::default()
    };
    model_treesitter::extract(&mut b, &root, &opts).unwrap();
    let index = model_scip::load(&index).unwrap();
    let st = model_link::link(&mut b, &index, &LinkOptions::default());
    let resolved = st.mapped + st.lifted;
    let ratio = resolved as f64 / (resolved + st.unmapped).max(1) as f64;
    eprintln!("{name}: {st:?} ratio={ratio:.3}");
    assert!(ratio > 0.9, "{name}: mapping ratio {ratio:.3} too low");
    Some((b.build(), st))
}

/// Outermost type enclosing `e` (itself included).
fn top_type(e: EntityRef<'_>) -> Option<EntityRef<'_>> {
    std::iter::once(e)
        .chain(e.ancestors())
        .filter(|a| a.kind().is_type())
        .last()
}

fn type_named<'m>(m: &'m Model, name: &str, file_suffix: &str) -> EntityRef<'m> {
    m.entities()
        .find(|e| {
            e.kind().is_type()
                && e.name() == name
                && e.file().is_some_and(|f| f.ends_with(file_suffix))
        })
        .unwrap_or_else(|| panic!("{name} not found"))
}

fn depends(m: &Model, from: EntityRef<'_>, to: EntityRef<'_>) -> bool {
    m.relations()
        .any(|r| top_type(r.from()) == Some(from) && top_type(r.to()) == Some(to))
}

#[test]
fn jpetstore() {
    let Some((m, _)) = build("jpetstore-6") else {
        return;
    };
    let p = "src/main/java/org/mybatis/jpetstore";
    let ctl = type_named(&m, "AccountController", "AccountController.java");
    let svc = type_named(&m, "AccountService", "AccountService.java");
    let mapper = type_named(&m, "AccountMapper", "AccountMapper.java");
    assert!(ctl.file().is_some_and(|f| f.starts_with(p)));
    assert!(depends(&m, ctl, svc));
    assert!(depends(&m, svc, mapper));
    // Matched entities carry their scip-java symbol.
    assert!(svc.key().starts_with("semanticdb maven "), "{}", svc.key());
    assert!(
        svc.key()
            .ends_with("org/mybatis/jpetstore/service/AccountService#")
    );
    // Tree-sitter props survive linking.
    assert!(svc.props().contains_key("annotations"), "{:?}", svc.props());
}

#[test]
fn angular_realworld() {
    let Some((m, _)) = build("angular-realworld") else {
        return;
    };
    let comp = type_named(&m, "FollowButtonComponent", "follow-button.component.ts");
    let svc = type_named(&m, "ProfileService", "profile.service.ts");
    assert!(depends(&m, comp, svc));
    let reaches_http = m.relations().any(|r| {
        top_type(r.from()) == Some(svc) && r.to().external() && r.to().key().contains("HttpClient#")
    });
    assert!(
        reaches_http,
        "ProfileService should reach the external HttpClient"
    );
    assert!(m.of_kind(EntityKind::Method).count() > 50);
}
