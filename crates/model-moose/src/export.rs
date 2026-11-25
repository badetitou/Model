//! Streams a [`Model`] as Moose/Famix JSON: one array, each object written
//! by hand with `"FM3"` first and `"id"` right after, references only as
//! `{"ref": n}`, nothing nested. See the crate README for the mapping; the
//! decisions in short:
//!
//! - **Entities.** A kind maps to an FM3 class through its `KindInfo::fm3`
//!   (kinds imported from Moose), else through
//!   [`mapping::EXPORT_KINDS`] by kind name. Kinds with neither (files,
//!   functions, plain variables) are not exported and counted in the
//!   report. Entity `i` gets Moose id `i + 1`; everything generated
//!   (associations, anchors, stubs) takes ids after the last entity.
//! - **Containment** goes through the container property chosen by the
//!   (child class, parent class) pair ([`mapping::EXPORT_CONTAINERS`]):
//!   `typeContainer`, `parentType`, `parentBehaviouralEntity`...
//!   Dotted package names are split into a chain of nested packages.
//! - **Relations** become reified associations, one per site (so each
//!   carries its own anchor): `calls` → `Invocation`, `accesses` →
//!   `Access` (`isWrite` from the site flags), `references` → `Reference`,
//!   `inherits` → `Inheritance`, `implements` → `Implementation`, `imports`
//!   → `Import`. `declared_type` becomes an `EntityTyping` referenced by the
//!   typed entity's `typing`; `throws` the method's `declaredExceptions`.
//!   `overrides` is derived in Famix and omitted.
//! - **Generated**: `isStub` for external entities; a stub `AnnotationType`
//!   for annotations whose type is not in the model; an `ImplicitVariable`
//!   `self` as receiver of calls a method makes to a method of its own
//!   enclosing type (an approximation: Model does not keep the receiver of
//!   a call); owners of type parameters become `Parametric*` classes.
//! - **Computed**: `numberOfLinesOfCode` (source range span) and
//!   `cyclomaticComplexity` (`model-analysis`; decision points come from
//!   frontends). Imported `moose.*` values take precedence.
//! - **Not written**: other derived properties, the association chain
//!   (`previous`/`next`), signatures (unless imported), receivers other than
//!   `self`, generics' arguments.
//!
//! # Source anchors
//!
//! Famix anchors are `IndexedFileAnchor`s with an absolute `fileName` and
//! character offsets: `startPos` is the 1-based offset of the first
//! character, `endPos` the 1-based offset of the last one (inclusive), the
//! same as the importer reads. Model keeps `(line, column)` with columns in
//! UTF-8 bytes and an exclusive end, so the writer reads every anchored
//! file once and converts: offset = characters before the position in the
//! file (a character is a Unicode scalar value, as Pharo counts; VerveineJ
//! counts UTF-16 units, which only differs after a character outside the
//! Basic Multilingual Plane). Files are looked up under
//! [`ExportOptions::root`] when the model's file names are relative. An
//! anchor whose file cannot be read is left out (and counted), the entity
//! stays.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use model_core::{EntityKind, EntityRef, Model, Range, RelationKind, SiteFlags, Value};

use crate::mapping;

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Directory the model's (relative) file names are under. Without it
    /// they are resolved against the current directory.
    pub root: Option<PathBuf>,
    /// Do not read source files: no anchors are written.
    pub no_sources: bool,
}

/// What the export wrote, by FM3 class, and what it left out and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// Objects written.
    pub objects: usize,
    /// Entities written, by FM3 class (simple name).
    pub entities: BTreeMap<String, usize>,
    /// Associations written, by FM3 class.
    pub associations: BTreeMap<String, usize>,
    pub anchors: usize,
    /// Anchors left out: no source file to read.
    pub anchors_skipped: usize,
    /// Entities of kinds with no FM3 class, by kind name.
    pub skipped_entities: BTreeMap<String, usize>,
    /// Relations (or parts of them) not written, by reason.
    pub skipped_relations: BTreeMap<String, usize>,
    /// Generated stub types (annotations).
    pub stubs: usize,
    /// Generated `self` variables.
    pub implicit_variables: usize,
    /// Parent packages made up from dotted names.
    pub synthesized_packages: usize,
    /// Files that could not be read.
    pub unreadable_files: usize,
}

impl ExportReport {
    pub fn entity_count(&self) -> usize {
        self.entities.values().sum()
    }

    pub fn association_count(&self) -> usize {
        self.associations.values().sum()
    }
}

impl fmt::Display for ExportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} objects: {} entities, {} associations, {} anchors ({} skipped, {} unreadable files), {} stubs, {} implicit variables, {} generated packages",
            self.objects,
            self.entity_count(),
            self.association_count(),
            self.anchors,
            self.anchors_skipped,
            self.unreadable_files,
            self.stubs,
            self.implicit_variables,
            self.synthesized_packages
        )?;
        let section = |f: &mut fmt::Formatter<'_>, title: &str, m: &BTreeMap<String, usize>| {
            if m.is_empty() {
                return Ok(());
            }
            writeln!(f, "{title}:")?;
            for (k, n) in m {
                writeln!(f, "  {n:>8}  {k}")?;
            }
            Ok(())
        };
        section(f, "entities", &self.entities)?;
        section(f, "associations", &self.associations)?;
        section(f, "entities not exported (kind)", &self.skipped_entities)?;
        section(f, "relations not exported", &self.skipped_relations)
    }
}

/// Writes `model` as Moose JSON to `out`.
pub fn export_json<W: Write>(
    model: &Model,
    out: W,
    options: &ExportOptions,
) -> io::Result<ExportReport> {
    let mut x = Exporter::new(model, BufWriter::new(out), options);
    x.run()?;
    x.w.flush()?;
    Ok(x.report)
}

/// [`export_json`] to a file.
pub fn export_path(
    model: &Model,
    path: impl AsRef<Path>,
    options: &ExportOptions,
) -> io::Result<ExportReport> {
    export_json(model, std::fs::File::create(path)?, options)
}

struct Cls {
    full: String,
    simple: String,
}

struct Exporter<'a, W: Write> {
    m: &'a Model,
    opts: &'a ExportOptions,
    w: W,
    report: ExportReport,
    /// Objects written so far (separators).
    written: usize,
    next_id: u64,
    classes: Vec<Cls>,
    class_index: HashMap<String, u16>,
    /// Per entity index: index+1 into `classes`, 0 when not exported.
    class_of: Vec<u16>,
    /// Top-level packages by qualified name → Moose id.
    packages: HashMap<String, u64>,
    /// Annotation types by name → entity index.
    annotation_types: HashMap<&'a str, usize>,
    stub_annotations: HashMap<String, u64>,
    /// First top-level type of each file: the importer of its imports.
    file_types: HashMap<&'a str, usize>,
    implicit_self: HashMap<usize, u64>,
    files: HashMap<String, Option<FileIndex>>,
    unreadable: std::collections::HashSet<String>,
}

type R = io::Result<()>;

impl<'a, W: Write> Exporter<'a, W> {
    fn new(m: &'a Model, w: W, opts: &'a ExportOptions) -> Self {
        Exporter {
            m,
            opts,
            w,
            report: ExportReport::default(),
            written: 0,
            next_id: m.entity_count() as u64 + 1,
            classes: Vec::new(),
            class_index: HashMap::new(),
            class_of: vec![0; m.entity_count()],
            packages: HashMap::new(),
            annotation_types: HashMap::new(),
            stub_annotations: HashMap::new(),
            file_types: HashMap::new(),
            implicit_self: HashMap::new(),
            files: HashMap::new(),
            unreadable: Default::default(),
        }
    }

    fn run(&mut self) -> R {
        self.classify();
        self.w.write_all(b"[\n")?;
        self.entities()?;
        self.associations()?;
        self.w.write_all(b"\n]\n")?;
        self.report.unreadable_files = self.unreadable.len();
        Ok(())
    }

    // ------------------------------------------------------------ classes

    fn intern(&mut self, full: String) -> u16 {
        if let Some(&i) = self.class_index.get(&full) {
            return i;
        }
        let simple = mapping::simple_name(&full).to_string();
        self.classes.push(Cls {
            full: full.clone(),
            simple,
        });
        let i = self.classes.len() as u16;
        self.class_index.insert(full, i);
        i
    }

    fn java(&mut self, simple: &str) -> u16 {
        self.intern(format!("{}.{simple}", mapping::JAVA_PACKAGE))
    }

    /// Assigns an FM3 class to every exportable entity.
    fn classify(&mut self) {
        let m = self.m;
        let mut by_kind: Vec<u16> = Vec::new();
        for (_, info) in m.metamodel().iter() {
            let class = match &info.fm3 {
                Some(full) => self.intern(full.clone()),
                None => match mapping::EXPORT_KINDS.iter().find(|(k, _)| *k == info.name) {
                    Some((_, simple)) => self.java(simple),
                    None => 0,
                },
            };
            by_kind.push(class);
        }
        let mut parametric = std::collections::HashSet::new();
        for e in m.entities() {
            let is_tp =
                m.metamodel().get(e.kind_id()).map(|k| k.name.as_str()) == Some("type_parameter");
            if let (true, Some(p)) = (is_tp, e.parent()) {
                parametric.insert(p.id().index());
            }
        }
        let thrown: std::collections::HashSet<usize> = m
            .relations()
            .filter(|r| matches!(r.kind(), RelationKind::Throws))
            .map(|r| r.to().id().index())
            .collect();
        for e in m.entities() {
            let i = e.id().index();
            let mut c = by_kind.get(e.kind_id().index()).copied().unwrap_or(0);
            if c == 0 {
                *self
                    .report
                    .skipped_entities
                    .entry(e.kind_name().to_string())
                    .or_default() += 1;
                continue;
            }
            if thrown.contains(&i) && self.classes[c as usize - 1].simple == "Class" {
                // Only `Exception` is throwable in Famix.
                c = self.java("Exception");
            } else if parametric.contains(&i) {
                let simple = self.classes[c as usize - 1].simple.clone();
                if let Some((_, p)) = mapping::PARAMETRIC.iter().find(|(s, _)| *s == simple) {
                    c = self.java(p);
                }
            }
            self.class_of[i] = c;
        }
        // Comments need a commentable parent, annotation instances any.
        for e in m.entities() {
            let i = e.id().index();
            let Some(simple) = self.simple_of(i) else {
                continue;
            };
            let drop = match simple {
                "Comment" => !e
                    .parent()
                    .and_then(|p| self.simple_of(p.id().index()))
                    .is_some_and(|p| mapping::COMMENTABLE.contains(&p)),
                "AnnotationInstance" => e.parent().is_none_or(|p| !self.exported(p.id().index())),
                _ => false,
            };
            if drop {
                self.class_of[i] = 0;
                *self
                    .report
                    .skipped_entities
                    .entry(e.kind_name().to_string())
                    .or_default() += 1;
            }
        }
        for e in m.entities() {
            let i = e.id().index();
            self.note_file_type(e);
            match self.simple_of(i) {
                Some("Package") if e.parent().is_none() => {
                    self.packages
                        .entry(e.name().to_string())
                        .or_insert(i as u64 + 1);
                }
                Some("AnnotationType") => {
                    let slot = self.annotation_types.entry(e.name()).or_insert(i);
                    if m.entity(model_core::EntityId::from_index(*slot as u32))
                        .is_some_and(|o| o.external())
                        && !e.external()
                    {
                        *slot = i;
                    }
                }
                _ => {}
            }
        }
    }

    /// Remembers the first top-level type of each source file.
    fn note_file_type(&mut self, e: EntityRef<'a>) {
        const TYPES: &[&str] = &[
            "Class",
            "ParametricClass",
            "Interface",
            "Enum",
            "AnnotationType",
        ];
        let Some(file) = e.file() else { return };
        if e.external()
            || !self
                .simple_of(e.id().index())
                .is_some_and(|s| TYPES.contains(&s))
        {
            return;
        }
        let nested = e.parent().is_some_and(|p| {
            self.simple_of(p.id().index())
                .is_some_and(|s| TYPES.contains(&s) || s.ends_with("Method"))
        });
        if !nested {
            self.file_types.entry(file).or_insert(e.id().index());
        }
    }

    fn exported(&self, i: usize) -> bool {
        self.class_of[i] != 0
    }

    fn simple_of(&self, i: usize) -> Option<&str> {
        match self.class_of[i] {
            0 => None,
            c => Some(self.classes[c as usize - 1].simple.as_str()),
        }
    }

    fn full_of(&self, i: usize) -> &str {
        self.classes[self.class_of[i] as usize - 1].full.as_str()
    }

    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // ------------------------------------------------------------- output

    fn begin(&mut self, class: &str, id: u64) -> R {
        if self.written > 0 {
            self.w.write_all(b",\n")?;
        }
        self.written += 1;
        self.report.objects += 1;
        self.w.write_all(b"{\"FM3\":")?;
        json_str(&mut self.w, class)?;
        write!(self.w, ",\"id\":{id}")
    }

    fn end(&mut self) -> R {
        self.w.write_all(b"}")
    }

    fn key(&mut self, k: &str) -> R {
        self.w.write_all(b",")?;
        json_str(&mut self.w, k)?;
        self.w.write_all(b":")
    }

    fn put_str(&mut self, k: &str, v: &str) -> R {
        self.key(k)?;
        json_str(&mut self.w, v)
    }

    fn put_bool(&mut self, k: &str, v: bool) -> R {
        self.key(k)?;
        self.w.write_all(if v { b"true" } else { b"false" })
    }

    fn put_int(&mut self, k: &str, v: i64) -> R {
        self.key(k)?;
        write!(self.w, "{v}")
    }

    fn put_ref(&mut self, k: &str, id: u64) -> R {
        self.key(k)?;
        write!(self.w, "{{\"ref\":{id}}}")
    }

    fn put_refs(&mut self, k: &str, ids: &[u64]) -> R {
        self.key(k)?;
        self.w.write_all(b"[")?;
        for (n, id) in ids.iter().enumerate() {
            if n > 0 {
                self.w.write_all(b",")?;
            }
            write!(self.w, "{{\"ref\":{id}}}")?;
        }
        self.w.write_all(b"]")
    }

    fn put_value(&mut self, k: &str, v: &Value) -> R {
        match v {
            Value::Bool(b) => self.put_bool(k, *b),
            Value::Int(i) => self.put_int(k, *i),
            Value::Str(s) => self.put_str(k, s),
            _ => Ok(()),
        }
    }

    fn count_entity(&mut self, class: &str) {
        *self
            .report
            .entities
            .entry(mapping::simple_name(class).to_string())
            .or_default() += 1;
    }

    fn count_assoc(&mut self, class: &str) {
        *self
            .report
            .associations
            .entry(class.to_string())
            .or_default() += 1;
    }

    fn skip_relation(&mut self, reason: &str) {
        *self
            .report
            .skipped_relations
            .entry(reason.to_string())
            .or_default() += 1;
    }

    // ----------------------------------------------------------- entities

    fn entities(&mut self) -> R {
        let m = self.m;
        for e in m.entities() {
            let i = e.id().index();
            if self.exported(i) {
                self.entity(e)?;
            }
        }
        Ok(())
    }

    /// Package `qualified` (a dotted name), made up when no entity is it.
    fn package(&mut self, qualified: &str) -> io::Result<u64> {
        if let Some(&id) = self.packages.get(qualified) {
            return Ok(id);
        }
        let id = self.alloc();
        self.packages.insert(qualified.to_string(), id);
        let (parent, name) = match qualified.rsplit_once('.') {
            Some((p, n)) => (Some(self.package(p)?), n),
            None => (None, qualified),
        };
        let class = format!("{}.Package", mapping::JAVA_PACKAGE);
        self.begin(&class, id)?;
        self.put_str("name", name)?;
        if let Some(p) = parent {
            self.put_ref("parentPackage", p)?;
        }
        self.end()?;
        self.count_entity(&class);
        self.report.synthesized_packages += 1;
        Ok(id)
    }

    fn entity(&mut self, e: EntityRef<'a>) -> R {
        let i = e.id().index();
        let id = i as u64 + 1;
        let full = self.full_of(i).to_string();
        let simple = mapping::simple_name(&full).to_string();

        // Container.
        let mut container: Option<(&'static str, u64)> = None;
        let mut name = e.name().to_string();
        // The nearest ancestor the child class can hang from.
        let fit = std::iter::successors(e.parent(), |p| p.parent()).find_map(|p| {
            let pi = p.id().index();
            let prop = mapping::export_container(&simple, self.simple_of(pi)?)?;
            Some((prop, pi as u64 + 1))
        });
        match fit {
            Some(c) => container = Some(c),
            None if simple == "Package" && e.parent().is_none() => {
                if let Some((prefix, last)) = e.name().rsplit_once('.') {
                    let (prefix, last) = (prefix.to_string(), last.to_string());
                    container = Some(("parentPackage", self.package(&prefix)?));
                    name = last;
                }
            }
            None => {}
        }

        // Typing and exceptions.
        let mut typing: Option<(u64, u64)> = None;
        let mut throws: Vec<u64> = Vec::new();
        for r in e.outgoing() {
            let to = r.to().id().index();
            if !self.exported(to) {
                continue;
            }
            match r.kind() {
                RelationKind::DeclaredType
                    if typing.is_none()
                        && (mapping::VARIABLE_CLASSES.contains(&simple.as_str())
                            || mapping::is_method(&simple))
                        && self
                            .simple_of(to)
                            .is_some_and(|c| mapping::TYPE_CLASSES.contains(&c)) =>
                {
                    typing = Some((self.alloc(), to as u64 + 1));
                }
                RelationKind::Throws
                    if mapping::is_method(&simple)
                        && self
                            .simple_of(to)
                            .is_some_and(|c| mapping::THROWABLE_CLASSES.contains(&c)) =>
                {
                    throws.push(to as u64 + 1)
                }
                _ => {}
            }
        }

        // Annotation instance: its type, a stub when unknown.
        let annotation_type = if simple == "AnnotationInstance" {
            Some(self.annotation_type(e.name())?)
        } else {
            None
        };

        self.begin(&full, id)?;
        if !matches!(simple.as_str(), "Comment" | "AnnotationInstance") && !name.is_empty() {
            self.put_str("name", &name)?;
        }
        if e.external() {
            self.put_bool("isStub", true)?;
        }
        self.primitives(e, &simple)?;
        if matches!(simple.as_str(), "Method" | "ParametricMethod") {
            let imported = e.props().get("moose.kind");
            if e.kind() == EntityKind::Constructor {
                self.put_str("kind", "constructor")?;
            } else if let Some(v) = imported {
                self.put_value("kind", v)?;
            }
            if let Some(v) = e.props().get("moose.signature") {
                self.put_value("signature", v)?;
            }
        }
        if let Some((prop, p)) = container {
            self.put_ref(prop, p)?;
            if simple == "TypeParameter" {
                self.put_ref("genericEntity", p)?;
            }
        }
        if let Some(t) = annotation_type {
            self.put_ref("annotationType", t)?;
        }
        if let Some((tid, _)) = typing {
            self.put_ref("typing", tid)?;
        }
        if !throws.is_empty() {
            self.put_refs("declaredExceptions", &throws)?;
        }
        self.end()?;
        self.count_entity(&full);

        if let Some((tid, target)) = typing {
            let class = format!("{}.EntityTyping", mapping::JAVA_PACKAGE);
            self.begin(&class, tid)?;
            self.put_ref("declaredType", target)?;
            self.end()?;
            self.count_assoc("EntityTyping");
        }
        if let (true, Some(file), Some(range)) = (simple != "Package", e.file(), e.range()) {
            self.anchor(id, file, range)?;
        }
        Ok(())
    }

    fn primitives(&mut self, e: EntityRef<'a>, simple: &str) -> R {
        for (prop, classes) in mapping::PRIMITIVE_PROPS {
            if !classes.contains(&simple) {
                continue;
            }
            let props = e.props();
            let v = props.get(&format!("moose.{prop}")).or_else(|| {
                (*prop == "visibility")
                    .then(|| props.get("visibility"))
                    .flatten()
            });
            if let Some(v) = v {
                self.put_value(prop, v)?;
                continue;
            }
            match *prop {
                "numberOfLinesOfCode" => {
                    self.put_int(prop, model_analysis::lines_of_code(e) as i64)?;
                }
                "cyclomaticComplexity" => {
                    let cyclo = model_analysis::cyclomatic_complexity(self.m, e.id());
                    self.put_int(prop, cyclo as i64)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Moose id of the annotation type called `name`; a stub is written
    /// the first time an unknown one is needed.
    fn annotation_type(&mut self, name: &str) -> io::Result<u64> {
        if let Some(&i) = self.annotation_types.get(name) {
            return Ok(i as u64 + 1);
        }
        if let Some(&id) = self.stub_annotations.get(name) {
            return Ok(id);
        }
        let id = self.alloc();
        self.stub_annotations.insert(name.to_string(), id);
        let class = format!("{}.AnnotationType", mapping::JAVA_PACKAGE);
        self.begin(&class, id)?;
        self.put_str("name", name)?;
        self.put_bool("isStub", true)?;
        self.end()?;
        self.count_entity(&class);
        self.report.stubs += 1;
        Ok(id)
    }

    // ------------------------------------------------------------ anchors

    fn absolute(&self, file: &str) -> PathBuf {
        let p = Path::new(file);
        let joined = match (&self.opts.root, p.is_absolute()) {
            (Some(root), false) => root.join(p),
            _ => p.to_path_buf(),
        };
        std::path::absolute(&joined).unwrap_or(joined)
    }

    fn file_index(&mut self, file: &str) -> Option<&FileIndex> {
        if !self.files.contains_key(file) {
            let path = self.absolute(file);
            let idx = if self.opts.no_sources {
                None
            } else {
                FileIndex::load(&path)
            };
            if idx.is_none() && !self.opts.no_sources {
                self.unreadable.insert(file.to_string());
            }
            self.files.insert(file.to_string(), idx);
        }
        self.files.get(file)?.as_ref()
    }

    /// Writes the anchor of the object `element`.
    fn anchor(&mut self, element: u64, file: &str, range: Range) -> R {
        let Some(idx) = self.file_index(file) else {
            self.report.anchors_skipped += 1;
            return Ok(());
        };
        let start = idx.offset(range.start_line, range.start_col) + 1;
        let end = idx.offset(range.end_line, range.end_col).max(start);
        let name = idx.name.clone();
        let id = self.alloc();
        let class = format!("{}.IndexedFileAnchor", mapping::JAVA_PACKAGE);
        self.begin(&class, id)?;
        self.put_ref("element", element)?;
        self.put_int("endPos", end as i64)?;
        self.put_str("fileName", &name)?;
        self.put_int("startPos", start as i64)?;
        self.end()?;
        self.report.anchors += 1;
        Ok(())
    }

    // ------------------------------------------------------- associations

    /// File whose offsets the sites of a relation from `e` are in.
    fn site_file(e: EntityRef<'a>) -> Option<&'a str> {
        e.file()
            .or_else(|| e.ancestors().find_map(|a| a.file()))
            .or_else(|| (e.kind() == EntityKind::File).then(|| e.name()))
    }

    /// The entity (or its nearest callable ancestor) that sends calls and
    /// makes accesses.
    fn owner(&self, e: EntityRef<'a>) -> Option<EntityRef<'a>> {
        if self.exported(e.id().index()) && e.kind().is_callable() {
            return Some(e);
        }
        e.ancestors()
            .find(|a| self.exported(a.id().index()) && a.kind().is_callable())
            .or_else(|| self.exported(e.id().index()).then_some(e))
    }

    /// The entity an import is written from: itself, or for a file the
    /// first type declared in it.
    fn importer(&self, e: EntityRef<'a>) -> Option<EntityRef<'a>> {
        if self.exported(e.id().index()) {
            return Some(e);
        }
        let file = e.file().unwrap_or_else(|| e.name());
        let i = *self.file_types.get(file)?;
        self.m.entity(model_core::EntityId::from_index(i as u32))
    }

    fn associations(&mut self) -> R {
        let m = self.m;
        for r in m.relations() {
            let (from, to) = (r.from(), r.to());
            let kind = r.kind();
            let class = match kind {
                RelationKind::Calls => "Invocation",
                RelationKind::Accesses => "Access",
                RelationKind::References => "Reference",
                RelationKind::Inherits => "Inheritance",
                RelationKind::Implements => "Implementation",
                RelationKind::Imports => "Import",
                RelationKind::DeclaredType | RelationKind::Throws => continue,
                RelationKind::Overrides => {
                    self.skip_relation("overrides (derived in Famix)");
                    continue;
                }
                other => {
                    self.skip_relation(&format!("{} (no Famix association)", other.as_str()));
                    continue;
                }
            };
            if !self.exported(to.id().index()) {
                self.skip_relation(&format!("{} to an entity not exported", kind.as_str()));
                continue;
            }
            let source = match kind {
                RelationKind::Calls | RelationKind::Accesses | RelationKind::References => {
                    self.owner(from)
                }
                RelationKind::Imports => self.importer(from),
                _ if self.exported(from.id().index()) => Some(from),
                _ => None,
            };
            let Some(source) = source else {
                self.skip_relation(&format!("{} from an entity not exported", kind.as_str()));
                continue;
            };
            let (Some(fs), Some(ts)) = (
                self.simple_of(source.id().index()),
                self.simple_of(to.id().index()),
            ) else {
                continue;
            };
            let class = match domain(class, fs, ts) {
                Ok(c) => c,
                Err(why) => {
                    self.skip_relation(&format!("{} {why}", kind.as_str()));
                    continue;
                }
            };
            let file = Self::site_file(from);
            let sites = r.sites();
            let flags = r.site_flags();
            let n = sites.len().max(1);
            for s in 0..n {
                let fl = flags.get(s).copied().unwrap_or(SiteFlags::NONE);
                self.association(class, source, to, fl)?;
                // The id just taken is the association's.
                let id = self.next_id - 1;
                match (sites.get(s), file) {
                    (Some(range), Some(file)) => self.anchor(id, file, *range)?,
                    (Some(_), None) => self.report.anchors_skipped += 1,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Writes one association object; its id is the last one allocated.
    fn association(
        &mut self,
        class: &str,
        from: EntityRef<'a>,
        to: EntityRef<'a>,
        flags: SiteFlags,
    ) -> R {
        let f = from.id().index() as u64 + 1;
        let t = to.id().index() as u64 + 1;
        // Receiver first: it may write the implicit variable.
        let receiver = if class == "Invocation" {
            self.receiver(from, to)?
        } else {
            None
        };
        let id = self.alloc();
        let full = format!("{}.{class}", mapping::JAVA_PACKAGE);
        self.begin(&full, id)?;
        match class {
            "Invocation" => {
                self.put_refs("candidates", &[t])?;
                if let Some(r) = receiver {
                    self.put_ref("receiver", r)?;
                }
                self.put_ref("sender", f)?;
            }
            "Access" => {
                self.put_ref("accessor", f)?;
                if flags.contains(SiteFlags::WRITE) {
                    self.put_bool("isWrite", true)?;
                }
                self.put_ref("variable", t)?;
            }
            "Reference" => {
                self.put_ref("referencer", f)?;
                self.put_ref("referredEntity", t)?;
            }
            "Inheritance" => {
                self.put_ref("subclass", f)?;
                self.put_ref("superclass", t)?;
            }
            "Implementation" => {
                self.put_ref("implementingClass", f)?;
                self.put_ref("interface", t)?;
            }
            _ => {
                self.put_ref("importedEntity", t)?;
                self.put_ref("importingEntity", f)?;
            }
        }
        self.end()?;
        self.count_assoc(class);
        Ok(())
    }

    /// `self` of `sender`, when the callee is a method of its own enclosing
    /// type (an unqualified call, as far as Model can tell).
    fn receiver(
        &mut self,
        sender: EntityRef<'a>,
        callee: EntityRef<'a>,
    ) -> io::Result<Option<u64>> {
        if callee.kind() != EntityKind::Method
            || !self
                .simple_of(sender.id().index())
                .is_some_and(|s| s.ends_with("Method"))
        {
            return Ok(None);
        }
        let same_type = match (sender.parent(), callee.parent()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if !same_type {
            return Ok(None);
        }
        let key = sender.id().index();
        if let Some(&id) = self.implicit_self.get(&key) {
            return Ok(Some(id));
        }
        let id = self.alloc();
        self.implicit_self.insert(key, id);
        let class = format!("{}.ImplicitVariable", mapping::JAVA_PACKAGE);
        self.begin(&class, id)?;
        self.put_str("name", "self")?;
        self.put_bool("isStub", true)?;
        self.put_ref("parentBehaviouralEntity", key as u64 + 1)?;
        self.end()?;
        self.count_entity(&class);
        self.report.implicit_variables += 1;
        Ok(Some(id))
    }
}

/// The association `class` between a `from` and a `to` class, if Famix
/// allows it: `Inheritance` and `Implementation` are told apart by the
/// target (a class implements an interface, anything else inherits).
fn domain(class: &'static str, from: &str, to: &str) -> Result<&'static str, &'static str> {
    use mapping::{IMPORT_CLASSES, INHERITING_CLASSES, TYPE_CLASSES, VARIABLE_CLASSES, is_method};
    let method = |c: &str| is_method(c);
    match class {
        "Invocation" if method(from) && method(to) => Ok(class),
        "Invocation" => Err("between non-methods (Famix invokes methods from methods)"),
        "Access" if method(from) && VARIABLE_CLASSES.contains(&to) => Ok(class),
        "Access" => Err("outside a method or of a non-variable"),
        "Reference" if method(from) && TYPE_CLASSES.contains(&to) => Ok(class),
        "Reference" => Err("outside a method or to a non-type (Famix: method to type)"),
        "Inheritance" | "Implementation" => {
            if is_method(from) && is_method(to) {
                return Err("between methods (overriding is derived in Famix)");
            }
            if !INHERITING_CLASSES.contains(&from) || !INHERITING_CLASSES.contains(&to) {
                return Err("between non-types");
            }
            let from_class = !from.ends_with("Interface") && from != "AnnotationType";
            let to_interface = to.ends_with("Interface") || to == "AnnotationType";
            if from_class && from != "Enum" && to_interface {
                Ok("Implementation")
            } else {
                Ok("Inheritance")
            }
        }
        "Import" if IMPORT_CLASSES.contains(&from) && IMPORT_CLASSES.contains(&to) => Ok(class),
        "Import" => Err("between entities that cannot import"),
        _ => Ok(class),
    }
}

/// A source file indexed for `(line, byte column)` → character offset.
struct FileIndex {
    /// Absolute path, as written in `fileName`.
    name: String,
    line_bytes: Vec<usize>,
    /// Characters before each line; only for non-ASCII text.
    line_chars: Vec<usize>,
    /// The text, kept only when it is not ASCII (offsets = bytes otherwise).
    text: Option<String>,
    len: usize,
}

impl FileIndex {
    fn load(path: &Path) -> Option<FileIndex> {
        let s = std::fs::read_to_string(path).ok()?;
        Some(FileIndex::new(path.to_string_lossy().into_owned(), s))
    }

    fn new(name: String, s: String) -> FileIndex {
        let mut line_bytes = vec![0];
        line_bytes.extend(s.match_indices('\n').map(|(i, _)| i + 1));
        let len = s.len();
        if s.is_ascii() {
            return FileIndex {
                name,
                line_bytes,
                line_chars: Vec::new(),
                text: None,
                len,
            };
        }
        let line_chars = line_bytes.iter().map(|&b| s[..b].chars().count()).collect();
        FileIndex {
            name,
            line_bytes,
            line_chars,
            text: Some(s),
            len,
        }
    }

    /// 0-based character offset of `(line, column in bytes)`; positions past
    /// the end are clamped to it.
    fn offset(&self, line: u32, col: u32) -> usize {
        let line = (line as usize).min(self.line_bytes.len() - 1);
        let start = self.line_bytes[line];
        let byte = (start + col as usize).min(self.len);
        match &self.text {
            None => byte,
            Some(t) => {
                let mut b = byte;
                while !t.is_char_boundary(b) {
                    b -= 1;
                }
                self.line_chars[line] + t[start..b].chars().count()
            }
        }
    }
}

/// Writes `s` as a JSON string.
fn json_str<W: Write>(w: &mut W, s: &str) -> R {
    w.write_all(b"\"")?;
    let mut from = 0;
    for (i, c) in s.char_indices() {
        let esc: Option<&str> = match c {
            '"' => Some("\\\""),
            '\\' => Some("\\\\"),
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            _ => None,
        };
        if esc.is_none() && c >= ' ' {
            continue;
        }
        w.write_all(&s.as_bytes()[from..i])?;
        match esc {
            Some(e) => w.write_all(e.as_bytes())?,
            None => write!(w, "\\u{:04x}", c as u32)?,
        }
        from = i + c.len_utf8();
    }
    w.write_all(&s.as_bytes()[from..])?;
    w.write_all(b"\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped() {
        let mut out = Vec::new();
        json_str(&mut out, "a\"b\\c\n\u{1}é").unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\"a\\\"b\\\\c\\n\\u0001é\""
        );
    }

    #[test]
    fn offsets_count_characters() {
        let f = FileIndex::new("f".into(), "é\nab".into());
        assert_eq!(f.offset(0, 0), 0);
        assert_eq!(f.offset(0, 2), 1);
        assert_eq!(f.offset(1, 0), 2);
        assert_eq!(f.offset(1, 2), 4);
        assert_eq!(f.offset(9, 9), 4);
        let a = FileIndex::new("f".into(), "ab\ncd".into());
        assert_eq!(a.offset(1, 1), 4);
    }
}
