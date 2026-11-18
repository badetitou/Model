//! SCIP index reader: decodes `index.scip` into a compact, language-neutral
//! form. Ported from Structure's `structure-scip`, keeping in addition what
//! a SCIP-only model needs (definition extents, enclosing symbols, kinds,
//! import roles).

mod kind;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use protobuf::{CodedInputStream, Message};
use rayon::prelude::*;
use scip::types;

use model_core::{EntityKind, Range};

pub use kind::entity_kind;

/// A SCIP symbol string, interned in its [`ScipIndex`] (see [`ScipIndex::name`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId(u32);

impl SymbolId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScipIndex {
    pub documents: Vec<ScipDocument>,
    /// From documents' symbol infos and `external_symbols`.
    pub symbols: HashMap<SymbolId, ScipSymbol>,
    /// Every symbol string once, by id: occurrences repeat a few symbols
    /// many times (vscode: 8.8M occurrences, 1M distinct symbols).
    names: Vec<Arc<str>>,
    ids: HashMap<Arc<str>, SymbolId>,
}

#[derive(Debug, Clone, Default)]
pub struct ScipDocument {
    pub relative_path: String,
    pub language: String,
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition {
    pub symbol: SymbolId,
    /// The identifier.
    pub range: Range,
    /// The whole definition (body included), when the indexer gives it.
    pub enclosing: Option<Range>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    pub symbol: SymbolId,
    pub range: Range,
    /// Occurrence in an import/use statement.
    pub import: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScipSymbol {
    pub display_name: Option<String>,
    pub kind: Option<EntityKind>,
    pub enclosing_symbol: Option<SymbolId>,
    pub relationships: Vec<ScipRelationship>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScipRelationship {
    pub symbol: SymbolId,
    pub is_implementation: bool,
    pub is_reference: bool,
    pub is_type_definition: bool,
    pub is_definition: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScipStats {
    pub documents: usize,
    pub symbols: usize,
    pub definitions: usize,
    pub references: usize,
}

const ROLE_DEFINITION: i32 = 0x1;
const ROLE_IMPORT: i32 = 0x2;

impl ScipIndex {
    /// The symbol string of `id`, which must come from this index.
    pub fn name(&self, id: SymbolId) -> &str {
        &self.names[id.index()]
    }

    /// Id of `name` if the index has seen it.
    pub fn id(&self, name: &str) -> Option<SymbolId> {
        self.ids.get(name).copied()
    }

    /// Number of distinct symbol strings; ids are `0..symbol_count()`.
    pub fn symbol_count(&self) -> usize {
        self.names.len()
    }

    pub fn ids(&self) -> impl ExactSizeIterator<Item = SymbolId> {
        (0..self.names.len() as u32).map(SymbolId)
    }

    /// Id of `name`, added to the index when new.
    pub fn intern(&mut self, name: &str) -> anyhow::Result<SymbolId> {
        if let Some(&id) = self.ids.get(name) {
            return Ok(id);
        }
        let id = SymbolId(u32::try_from(self.names.len()).context("too many SCIP symbols")?);
        let name: Arc<str> = Arc::from(name);
        self.names.push(name.clone());
        self.ids.insert(name, id);
        Ok(id)
    }

    /// Symbol information of `name`, if any.
    pub fn symbol(&self, name: &str) -> Option<&ScipSymbol> {
        self.symbols.get(&self.id(name)?)
    }

    pub fn stats(&self) -> ScipStats {
        ScipStats {
            documents: self.documents.len(),
            symbols: self.symbols.len(),
            definitions: self.documents.iter().map(|d| d.definitions.len()).sum(),
            references: self.documents.iter().map(|d| d.references.len()).sum(),
        }
    }
}

pub fn is_local(symbol: &str) -> bool {
    symbol.starts_with("local ")
}

pub fn load(path: &Path) -> anyhow::Result<ScipIndex> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let mut input = CodedInputStream::new(&mut reader);
    // Documents are decoded in batches, so only a bounded amount of raw
    // protobuf sits in memory next to the result.
    decode(&mut input, BATCH_BYTES).with_context(|| format!("decoding {}", path.display()))
}

pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<ScipIndex> {
    decode(&mut CodedInputStream::from_bytes(bytes), BATCH_BYTES)
}

/// Calls `f` with each still-encoded `Document` of the index at `path`, in
/// file order. External symbols and metadata are skipped.
pub fn for_each_raw_document(
    path: &Path,
    f: &mut dyn FnMut(Vec<u8>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let mut input = CodedInputStream::new(&mut reader);
    while let Some(tag) = input
        .read_raw_tag_or_eof()
        .context("invalid SCIP protobuf")?
    {
        if tag >> 3 == 2 {
            let len = input.read_raw_varint32().context("invalid SCIP protobuf")?;
            f(input.read_raw_bytes(len).context("invalid SCIP protobuf")?)?;
        } else {
            skip(&mut input, tag)?;
        }
    }
    Ok(())
}

/// `relative_path` of an encoded `Document`, without decoding the rest.
pub fn raw_document_path(raw: &[u8]) -> anyhow::Result<String> {
    let mut input = CodedInputStream::from_bytes(raw);
    let mut path = String::new();
    while let Some(tag) = input
        .read_raw_tag_or_eof()
        .context("invalid SCIP protobuf")?
    {
        if tag == (1 << 3 | 2) {
            // Last value wins, as in a full decode.
            path = input.read_string().context("invalid SCIP protobuf")?;
        } else {
            skip(&mut input, tag)?;
        }
    }
    Ok(path)
}

/// Index made of encoded `Document`s (e.g. from [`for_each_raw_document`]),
/// decoded in bounded batches like [`load`].
pub fn from_raw_documents(
    docs: impl IntoIterator<Item = anyhow::Result<Vec<u8>>>,
) -> anyhow::Result<ScipIndex> {
    let mut out = ScipIndex::default();
    let mut batch: Vec<Vec<u8>> = Vec::new();
    let mut batch_bytes = 0usize;
    for raw in docs {
        let raw = raw?;
        batch_bytes += raw.len();
        batch.push(raw);
        if batch_bytes >= BATCH_BYTES {
            decode_batch(&mut batch, &mut out)?;
            batch_bytes = 0;
        }
    }
    decode_batch(&mut batch, &mut out)?;
    Ok(out)
}

fn skip(input: &mut CodedInputStream, tag: u32) -> anyhow::Result<()> {
    let wt = protobuf::rt::WireType::new(tag & 7).context("invalid SCIP protobuf")?;
    input.skip_field(wt).context("invalid SCIP protobuf")
}

/// Streams the top-level `Index` message (`documents` = 2,
/// `external_symbols` = 3; metadata and unknown fields are skipped).
fn decode(input: &mut CodedInputStream, batch_limit: usize) -> anyhow::Result<ScipIndex> {
    let mut out = ScipIndex::default();
    let mut external: Vec<types::SymbolInformation> = Vec::new();
    let mut batch: Vec<Vec<u8>> = Vec::new();
    let mut batch_bytes = 0usize;
    while let Some(tag) = input
        .read_raw_tag_or_eof()
        .context("invalid SCIP protobuf")?
    {
        match tag >> 3 {
            2 => {
                // Raw document bytes are batched, then decoded in parallel
                // (in order); only one batch of raw bytes is ever resident.
                let len = input.read_raw_varint32().context("invalid SCIP protobuf")?;
                let raw = input.read_raw_bytes(len).context("invalid SCIP protobuf")?;
                batch_bytes += raw.len();
                batch.push(raw);
                if batch_bytes >= batch_limit {
                    decode_batch(&mut batch, &mut out)?;
                    batch_bytes = 0;
                }
            }
            3 => external.push(input.read_message().context("invalid SCIP protobuf")?),
            _ => skip(input, tag)?,
        }
    }
    decode_batch(&mut batch, &mut out)?;
    for info in &external {
        add_symbol(&mut out, info)?;
    }
    Ok(out)
}

/// Raw (still encoded) document bytes per parallel batch; the decoded batch
/// is larger than this.
const BATCH_BYTES: usize = 64 << 20;

/// An occurrence before its symbol is interned: index into the document's
/// local names.
enum Occ {
    Def(u32, Range, Option<Range>),
    Ref(u32, Range, bool),
}

/// One document decoded on a worker thread, its symbols deduplicated
/// locally before they are interned (in document order) into the index.
struct DecodedDocument {
    relative_path: String,
    language: String,
    names: Vec<String>,
    occurrences: Vec<Occ>,
    infos: Vec<types::SymbolInformation>,
}

fn decode_document(raw: &[u8]) -> anyhow::Result<DecodedDocument> {
    let doc = types::Document::parse_from_bytes(raw).context("invalid SCIP protobuf")?;
    let mut local: HashMap<String, u32> = HashMap::new();
    let mut occurrences = Vec::with_capacity(doc.occurrences.len());
    for o in doc.occurrences {
        if o.symbol.is_empty() {
            continue;
        }
        let Some(range) = convert_range(&o.range) else {
            continue;
        };
        let next = local.len() as u32;
        let i = *local.entry(o.symbol).or_insert(next);
        occurrences.push(if o.symbol_roles & ROLE_DEFINITION != 0 {
            Occ::Def(i, range, convert_range(&o.enclosing_range))
        } else {
            Occ::Ref(i, range, o.symbol_roles & ROLE_IMPORT != 0)
        });
    }
    let mut names = vec![String::new(); local.len()];
    for (name, i) in local {
        names[i as usize] = name;
    }
    Ok(DecodedDocument {
        relative_path: doc.relative_path,
        language: doc.language,
        names,
        occurrences,
        infos: doc.symbols,
    })
}

/// Decodes the batched documents in parallel and appends them to `out` in
/// their original order, then empties the batch.
fn decode_batch(batch: &mut Vec<Vec<u8>>, out: &mut ScipIndex) -> anyhow::Result<()> {
    let decoded: Vec<anyhow::Result<DecodedDocument>> =
        batch.par_iter().map(|raw| decode_document(raw)).collect();
    batch.clear();
    for d in decoded {
        let d = d?;
        for info in &d.infos {
            add_symbol(out, info)?;
        }
        let ids = d
            .names
            .iter()
            .map(|n| out.intern(n))
            .collect::<anyhow::Result<Vec<SymbolId>>>()?;
        let mut doc = ScipDocument {
            relative_path: d.relative_path,
            language: d.language,
            ..ScipDocument::default()
        };
        for o in d.occurrences {
            match o {
                Occ::Def(i, range, enclosing) => doc.definitions.push(Definition {
                    symbol: ids[i as usize],
                    range,
                    enclosing,
                }),
                Occ::Ref(i, range, import) => doc.references.push(Reference {
                    symbol: ids[i as usize],
                    range,
                    import,
                }),
            }
        }
        doc.definitions.shrink_to_fit();
        doc.references.shrink_to_fit();
        out.documents.push(doc);
    }
    Ok(())
}

fn add_symbol(out: &mut ScipIndex, info: &types::SymbolInformation) -> anyhow::Result<()> {
    if info.symbol.is_empty() {
        return Ok(());
    }
    let relationships = info
        .relationships
        .iter()
        .map(|r| {
            Ok(ScipRelationship {
                symbol: out.intern(&r.symbol)?,
                is_implementation: r.is_implementation,
                is_reference: r.is_reference,
                is_type_definition: r.is_type_definition,
                is_definition: r.is_definition,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let enclosing_symbol = if info.enclosing_symbol.is_empty() {
        None
    } else {
        Some(out.intern(&info.enclosing_symbol)?)
    };
    let new = ScipSymbol {
        display_name: (!info.display_name.is_empty()).then(|| info.display_name.clone()),
        kind: info.kind.enum_value().ok().and_then(entity_kind),
        enclosing_symbol,
        relationships,
    };
    let id = out.intern(&info.symbol)?;
    match out.symbols.get_mut(&id) {
        // Same symbol seen twice: merge relationships, fill gaps.
        Some(old) => {
            old.relationships.extend(new.relationships);
            old.display_name = old.display_name.take().or(new.display_name);
            old.kind = old.kind.or(new.kind);
            old.enclosing_symbol = old.enclosing_symbol.or(new.enclosing_symbol);
        }
        None => {
            out.symbols.insert(id, new);
        }
    }
    Ok(())
}

/// `[sl, sc, ec]` or `[sl, sc, el, ec]`; `None` if absent or malformed.
fn convert_range(r: &[i32]) -> Option<Range> {
    let u = |i: i32| u32::try_from(i).ok();
    let (sl, sc, el, ec) = match *r {
        [sl, sc, ec] => (u(sl)?, u(sc)?, u(sl)?, u(ec)?),
        [sl, sc, el, ec] => (u(sl)?, u(sc)?, u(el)?, u(ec)?),
        _ => return None,
    };
    if (el, ec) < (sl, sc) {
        return None;
    }
    Some(Range::new(sl, sc, el, ec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{Document, Index, Occurrence, Relationship, SymbolInformation};

    const JAVA: &str = "semanticdb maven maven/org.mybatis/jpetstore 6.1.0 org/mybatis/jpetstore/service/AccountService#getAccount().";

    fn occ(sym: &str, range: Vec<i32>, roles: i32) -> Occurrence {
        Occurrence {
            symbol: sym.into(),
            range,
            symbol_roles: roles,
            ..Default::default()
        }
    }

    #[test]
    fn decodes_definitions_references_and_symbols() {
        use types::symbol_information::Kind;
        let mut idx = Index::new();
        let mut d1 = Document::new();
        d1.relative_path = "A.java".into();
        d1.language = "java".into();
        let mut def = occ(JAVA, vec![1, 2, 10], ROLE_DEFINITION);
        def.enclosing_range = vec![1, 0, 3, 1];
        d1.occurrences.push(def);
        d1.occurrences.push(occ("", vec![0, 0, 1], 0));
        d1.occurrences.push(occ("local 1", vec![0, 0, 1, 5], 0));
        d1.occurrences.push(occ(JAVA, vec![1, 2], 0));
        d1.occurrences.push(occ("x", vec![0, 7, 9], ROLE_IMPORT));
        let mut info = SymbolInformation::new();
        info.symbol = JAVA.into();
        info.display_name = "getAccount".into();
        info.kind = Kind::StaticMethod.into();
        info.enclosing_symbol = "enc".into();
        let mut rel = Relationship::new();
        rel.symbol = "x".into();
        rel.is_implementation = true;
        info.relationships.push(rel);
        d1.symbols.push(info);
        let mut d2 = Document::new();
        d2.relative_path = "B.java".into();
        d2.occurrences.push(occ(JAVA, vec![4, 1, 9], 0));
        idx.documents.push(d1);
        idx.documents.push(d2);
        let mut ext = SymbolInformation::new();
        ext.symbol = "ext 1".into();
        idx.external_symbols.push(ext);

        let m = from_bytes(&idx.write_to_bytes().unwrap()).unwrap();
        assert_eq!(
            m.stats(),
            ScipStats {
                documents: 2,
                symbols: 2,
                definitions: 1,
                references: 3
            }
        );
        let d = &m.documents[0];
        assert_eq!(d.definitions[0].range, Range::new(1, 2, 1, 10));
        assert_eq!(d.definitions[0].enclosing, Some(Range::new(1, 0, 3, 1)));
        // Empty-symbol and malformed (2-element) occurrences are dropped.
        let refs: Vec<_> = d
            .references
            .iter()
            .map(|r| (m.name(r.symbol), r.import))
            .collect();
        assert_eq!(refs, [("local 1", false), ("x", true)]);
        let s = m.symbol(JAVA).unwrap();
        assert_eq!(s.display_name.as_deref(), Some("getAccount"));
        assert_eq!(s.kind, Some(EntityKind::Method));
        assert_eq!(s.enclosing_symbol.map(|e| m.name(e)), Some("enc"));
        assert!(s.relationships[0].is_implementation);
        assert_eq!(m.name(s.relationships[0].symbol), "x");
        assert!(m.symbol("ext 1").is_some());
        // One id per distinct symbol, shared by every occurrence.
        assert_eq!(
            m.documents[0].definitions[0].symbol,
            m.documents[1].references[0].symbol
        );
    }

    #[test]
    fn raw_documents_roundtrip() {
        let mut idx = Index::new();
        for (p, line) in [("A.java", 1), ("dir/B.java", 4)] {
            let mut d = Document::new();
            d.occurrences.push(occ(JAVA, vec![line, 1, 9], 0));
            // Path written after the occurrences: still found.
            d.relative_path = p.into();
            idx.documents.push(d);
        }
        let mut ext = SymbolInformation::new();
        ext.symbol = "ext 1".into();
        idx.external_symbols.push(ext);
        let file = std::env::temp_dir().join(format!("model-scip-raw-{}.scip", std::process::id()));
        std::fs::write(&file, idx.write_to_bytes().unwrap()).unwrap();
        let mut raws = Vec::new();
        for_each_raw_document(&file, &mut |r| {
            raws.push(r);
            Ok(())
        })
        .unwrap();
        let _ = std::fs::remove_file(&file);
        let paths: Vec<String> = raws.iter().map(|r| raw_document_path(r).unwrap()).collect();
        assert_eq!(paths, ["A.java", "dir/B.java"]);
        // Only the second document, recomposed: positions kept, externals gone.
        let m = from_raw_documents([Ok(raws[1].clone())]).unwrap();
        assert_eq!(m.documents.len(), 1);
        assert_eq!(m.documents[0].relative_path, "dir/B.java");
        assert_eq!(m.documents[0].references[0].range.start_line, 4);
        assert!(m.symbols.is_empty());
        assert!(raw_document_path(&[0xff, 0xff]).is_err());
    }

    #[test]
    fn invalid_bytes_error() {
        assert!(from_bytes(&[0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn symbol_merge() {
        use types::symbol_information::Kind;
        let rel = |s: &str| {
            let mut r = Relationship::new();
            r.symbol = s.into();
            r.is_reference = true;
            r
        };
        let mut doc_info = SymbolInformation::new();
        doc_info.symbol = "s 1".into();
        doc_info.relationships.push(rel("a"));
        // display_name empty and kind unspecified here: filled by external.
        let mut ext = SymbolInformation::new();
        ext.symbol = "s 1".into();
        ext.display_name = "First".into();
        ext.kind = Kind::Class.into();
        ext.relationships.push(rel("b"));
        let mut ext2 = SymbolInformation::new();
        ext2.symbol = "s 1".into();
        ext2.display_name = "Second".into();
        ext2.kind = Kind::Method.into();

        let mut idx = Index::new();
        let mut d = Document::new();
        d.symbols.push(doc_info);
        idx.documents.push(d);
        idx.external_symbols.push(ext);
        idx.external_symbols.push(ext2);
        let m = from_bytes(&idx.write_to_bytes().unwrap()).unwrap();
        assert_eq!(m.symbols.len(), 1);
        let s = m.symbol("s 1").unwrap();
        let rels: Vec<_> = s.relationships.iter().map(|r| m.name(r.symbol)).collect();
        assert_eq!(rels, ["a", "b"]);
        assert_eq!(s.display_name.as_deref(), Some("First"));
        assert_eq!(s.kind, Some(EntityKind::Class));
    }

    #[test]
    fn ranges() {
        assert_eq!(convert_range(&[3, 4, 9]), Some(Range::new(3, 4, 3, 9)));
        assert_eq!(convert_range(&[3, 4, 5, 2]), Some(Range::new(3, 4, 5, 2)));
        assert!(convert_range(&[]).is_none());
        assert!(convert_range(&[1, 2]).is_none());
        assert!(convert_range(&[1, 2, 3, 4, 5]).is_none());
        assert!(convert_range(&[-1, 2, 3]).is_none());
        assert!(convert_range(&[3, 4, 2, 9]).is_none());
    }

    fn many_docs_index(n: usize) -> Vec<u8> {
        let mut idx = Index::new();
        for i in 0..n {
            let mut d = Document::new();
            d.relative_path = format!("F{i}.java");
            d.language = "java".into();
            for j in 0..=(i % 3) {
                d.occurrences
                    .push(occ(JAVA, vec![j as i32, 0, 5], (j % 2) as i32));
            }
            let mut info = SymbolInformation::new();
            info.symbol = format!("{JAVA}{i}");
            d.symbols.push(info);
            idx.documents.push(d);
        }
        idx.write_to_bytes().unwrap()
    }

    fn summary(m: &ScipIndex) -> (Vec<(String, usize, usize)>, usize) {
        (
            m.documents
                .iter()
                .map(|d| {
                    (
                        d.relative_path.clone(),
                        d.definitions.len(),
                        d.references.len(),
                    )
                })
                .collect(),
            m.symbols.len(),
        )
    }

    /// A tiny batch limit forces many batches (and a final partial one); the
    /// result, document order included, matches the single-batch decode.
    #[test]
    fn batched_decode_matches_single_batch() {
        let bytes = many_docs_index(11);
        let one = decode(&mut CodedInputStream::from_bytes(&bytes), usize::MAX).unwrap();
        for limit in [1, 40, 100, 100_000] {
            let m = decode(&mut CodedInputStream::from_bytes(&bytes), limit).unwrap();
            assert_eq!(summary(&m), summary(&one), "limit {limit}");
        }
        let names: Vec<&str> = one
            .documents
            .iter()
            .map(|d| d.relative_path.as_str())
            .collect();
        let want: Vec<String> = (0..11).map(|i| format!("F{i}.java")).collect();
        assert_eq!(names, want.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(one.symbols.len(), 11);
    }

    /// A valid frame around an undecodable document is an error, not a panic.
    #[test]
    fn corrupt_document_body_is_an_error() {
        let mut bytes = many_docs_index(2);
        // Field 2 (document), length 3, then a truncated varint / bad tag.
        bytes.extend_from_slice(&[0x12, 0x03, 0x0A, 0xFF, 0xFF]);
        for limit in [1, usize::MAX] {
            let r = decode(&mut CodedInputStream::from_bytes(&bytes), limit);
            assert!(r.is_err(), "limit {limit}");
        }
    }
}
