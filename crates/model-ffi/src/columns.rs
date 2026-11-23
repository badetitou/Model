//! Safe construction of the flat columns exposed over the C ABI.
//!
//! Everything is read through `model-core`'s public views, so the frozen
//! model keeps its compact private layout. Strings are not copied: a
//! [`StrRef`] points into the model's own buffers, valid while the model is
//! alive and unchanged.

use std::collections::HashMap;

use model_core::{Model, Provenance};

/// Absent reference (parent, language) in a `u32` column.
pub const NONE: u32 = u32::MAX;

/// Entity flag bits (`entity_flags` column).
pub const FLAG_EXTERNAL: u32 = 1;
pub const FLAG_HAS_RANGE: u32 = 2;
pub const FLAG_HAS_NAME_RANGE: u32 = 4;
pub const FLAG_HAS_PROPS: u32 = 8;

/// A borrowed UTF-8 string. `ptr` is null for an absent string.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StrRef {
    pub ptr: *const u8,
    pub len: u32,
}

impl StrRef {
    pub(crate) const ABSENT: StrRef = StrRef {
        ptr: std::ptr::null(),
        len: 0,
    };

    pub(crate) fn of(s: &str) -> StrRef {
        StrRef {
            ptr: s.as_ptr(),
            len: u32::try_from(s.len()).unwrap_or(u32::MAX),
        }
    }

    pub(crate) fn opt(s: Option<&str>) -> StrRef {
        s.map_or(StrRef::ABSENT, StrRef::of)
    }
}

/// Index of a provenance in the `relation_provenances` column.
pub fn provenance_code(p: Provenance) -> u32 {
    match p {
        Provenance::TreeSitter => 0,
        Provenance::Scip => 1,
        Provenance::Frontend => 2,
        Provenance::Derived => 3,
    }
}

/// Every column, built once when a model is opened. Ranges are flat
/// `[start_line, start_col, end_line, end_col]` quadruples (0-based, end
/// exclusive, columns in UTF-8 bytes); an absent range is four zeros, told
/// apart by the flags.
#[derive(Default)]
pub struct Columns {
    pub entity_keys: Vec<StrRef>,
    pub entity_names: Vec<StrRef>,
    pub entity_files: Vec<StrRef>,
    pub entity_parents: Vec<u32>,
    pub entity_kinds: Vec<u32>,
    pub entity_languages: Vec<u32>,
    pub entity_flags: Vec<u32>,
    pub entity_ranges: Vec<u32>,
    pub entity_name_ranges: Vec<u32>,

    pub relation_from: Vec<u32>,
    pub relation_to: Vec<u32>,
    pub relation_kinds: Vec<u32>,
    pub relation_provenances: Vec<u32>,
    /// `site_offsets[r]..site_offsets[r + 1]` are the sites of relation `r`.
    pub site_offsets: Vec<u32>,
    pub site_ranges: Vec<u32>,
    pub site_flags: Vec<u8>,

    pub kind_names: Vec<StrRef>,
    pub kind_archetypes: Vec<StrRef>,
    pub kind_fm3: Vec<StrRef>,
    pub kind_traits: Vec<u64>,
    pub language_names: Vec<StrRef>,
    pub relation_kind_names: Vec<StrRef>,
}

fn push_range(out: &mut Vec<u32>, r: Option<model_core::Range>) {
    let r = r.unwrap_or_default();
    out.extend([r.start_line, r.start_col, r.end_line, r.end_col]);
}

impl Columns {
    pub fn build(model: &Model) -> Columns {
        let mut c = Columns::default();

        let languages: HashMap<&str, u32> = model
            .languages()
            .iter()
            .enumerate()
            .map(|(i, l)| (l.as_str(), i as u32))
            .collect();
        c.language_names = model
            .languages()
            .iter()
            .map(|l| StrRef::of(l.as_str()))
            .collect();

        let n = model.entity_count();
        c.entity_keys.reserve(n);
        c.entity_names.reserve(n);
        c.entity_files.reserve(n);
        c.entity_parents.reserve(n);
        c.entity_kinds.reserve(n);
        c.entity_languages.reserve(n);
        c.entity_flags.reserve(n);
        c.entity_ranges.reserve(4 * n);
        c.entity_name_ranges.reserve(4 * n);
        for e in model.entities() {
            c.entity_keys.push(StrRef::of(e.key()));
            c.entity_names.push(StrRef::of(e.name()));
            c.entity_files.push(StrRef::opt(e.file()));
            c.entity_parents
                .push(e.parent().map_or(NONE, |p| p.id().index() as u32));
            c.entity_kinds.push(e.kind_id().index() as u32);
            c.entity_languages.push(
                e.language()
                    .and_then(|l| languages.get(l.as_str()).copied())
                    .unwrap_or(NONE),
            );
            let (range, name_range) = (e.range(), e.name_range());
            let mut flags = 0;
            if e.external() {
                flags |= FLAG_EXTERNAL;
            }
            if range.is_some() {
                flags |= FLAG_HAS_RANGE;
            }
            if name_range.is_some() {
                flags |= FLAG_HAS_NAME_RANGE;
            }
            if !e.props().is_empty() {
                flags |= FLAG_HAS_PROPS;
            }
            c.entity_flags.push(flags);
            push_range(&mut c.entity_ranges, range);
            push_range(&mut c.entity_name_ranges, name_range);
        }

        // Kinds are dense by id; `iter` yields them in id order.
        for (_, info) in model.metamodel().iter() {
            c.kind_names.push(StrRef::of(&info.name));
            c.kind_archetypes.push(StrRef::of(info.archetype.as_str()));
            c.kind_fm3.push(StrRef::opt(info.fm3.as_deref()));
            c.kind_traits.push(info.traits.0);
        }

        let mut relation_kinds: HashMap<&str, u32> = HashMap::new();
        c.site_offsets.push(0);
        for r in model.relations() {
            c.relation_from.push(r.from().id().index() as u32);
            c.relation_to.push(r.to().id().index() as u32);
            let name = r.kind().as_str();
            let next = relation_kinds.len() as u32;
            let code = *relation_kinds.entry(name).or_insert_with(|| {
                c.relation_kind_names.push(StrRef::of(name));
                next
            });
            c.relation_kinds.push(code);
            c.relation_provenances.push(provenance_code(r.provenance()));
            for s in r.sites() {
                c.site_ranges
                    .extend([s.start_line, s.start_col, s.end_line, s.end_col]);
            }
            c.site_flags.extend(r.site_flags().iter().map(|f| f.0));
            c.site_offsets.push(c.site_flags.len() as u32);
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_core::{EntityKind, LanguageId, ModelBuilder, Range, RelationKind};

    fn sample() -> Model {
        let java = LanguageId::new("java");
        let mut b = ModelBuilder::new();
        let pkg = b.entity("k pkg", EntityKind::Namespace, "acme");
        let a = b.entity("k A", EntityKind::Class, "A");
        let run = b.entity("k run", EntityKind::Method, "run");
        let list = b.entity("k List", EntityKind::Interface, "List");
        for e in [pkg, a, run] {
            b.set_language(e, &java);
        }
        b.set_parent(a, Some(pkg));
        b.set_parent(run, Some(a));
        b.set_source(
            a,
            "src/A.java",
            Range::new(2, 0, 20, 1),
            Some(Range::new(2, 13, 2, 14)),
        );
        b.set_external(list, true);
        b.set_prop(a, "java.annotations", vec!["Service"]);
        let site = |l| Range::new(l, 0, l, 1);
        b.occurrence(&RelationKind::Calls, run, list, Provenance::Scip, site(5));
        b.occurrence(&RelationKind::Calls, run, list, Provenance::Scip, site(7));
        b.occurrence(&RelationKind::Calls, run, a, Provenance::Scip, site(8));
        b.build()
    }

    #[test]
    fn columns_match_the_views() {
        let m = sample();
        let c = Columns::build(&m);
        let n = m.entity_count();
        assert_eq!(c.entity_keys.len(), n);
        assert_eq!(c.entity_ranges.len(), 4 * n);
        for e in m.entities() {
            let i = e.id().index();
            // SAFETY: the StrRef points into `m`, which is alive.
            let key = unsafe {
                std::str::from_utf8(std::slice::from_raw_parts(
                    c.entity_keys[i].ptr,
                    c.entity_keys[i].len as usize,
                ))
            };
            assert_eq!(key, Ok(e.key()));
            assert_eq!(
                c.entity_parents[i],
                e.parent().map_or(NONE, |p| p.id().index() as u32)
            );
            assert_eq!(c.entity_flags[i] & FLAG_EXTERNAL != 0, e.external());
            assert_eq!(c.entity_flags[i] & FLAG_HAS_RANGE != 0, e.range().is_some());
            assert_eq!(
                c.entity_flags[i] & FLAG_HAS_PROPS != 0,
                !e.props().is_empty()
            );
        }
        let a = m.by_key("k A").map(|e| e.id().index());
        assert_eq!(
            a.map(|i| &c.entity_ranges[4 * i..4 * i + 4]),
            Some(&[2, 0, 20, 1][..])
        );
    }

    #[test]
    fn relations_and_sites_are_csr() {
        let m = sample();
        let c = Columns::build(&m);
        assert_eq!(c.relation_from.len(), m.relation_count());
        assert_eq!(c.site_offsets.len(), m.relation_count() + 1);
        assert_eq!(c.site_flags.len(), m.site_count());
        assert_eq!(c.site_ranges.len(), 4 * m.site_count());
        assert_eq!(c.site_offsets.last().copied(), Some(m.site_count() as u32));
        assert_eq!(c.relation_kind_names.len(), 1);
        assert_eq!(
            c.relation_provenances,
            vec![provenance_code(Provenance::Scip); 2]
        );
    }

    #[test]
    fn kinds_and_languages_are_tables() {
        let m = sample();
        let c = Columns::build(&m);
        assert_eq!(c.kind_names.len(), m.metamodel().len());
        assert_eq!(c.kind_fm3.len(), c.kind_names.len());
        assert_eq!(c.language_names.len(), 1);
        let java = m.by_key("k A").map(|e| e.id().index());
        assert_eq!(java.map(|i| c.entity_languages[i]), Some(0));
        let ext = m.by_key("k List").map(|e| e.id().index());
        assert_eq!(ext.map(|i| c.entity_languages[i]), Some(NONE));
    }
}
