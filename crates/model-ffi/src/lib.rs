//! C ABI over a frozen [`Model`](model_core::Model), for foreign importers
//! (Pharo through UFFI, ...).
//!
//! Depends on `model-core` and `model-analysis` (plus `model-factory`
//! behind the optional `build` feature): it exposes raw columns, tables and
//! derived code metrics, with no knowledge of any language or of Famix.
//! Mapping to a target metamodel
//! (FM3 class per kind, containers, associations) is the client's business,
//! driven by [`KindInfo`](model_core::KindInfo) (`kind_fm3`).
//!
//! # Usage
//!
//! `model_open` loads a model JSON and returns an opaque handle (null on
//! error, see `model_last_error`); with the `build` feature, `model_build`
//! builds the model from sources in-process and returns the same kind of
//! handle, with no JSON involved. Every `model_*` column getter returns a
//! pointer into the handle's memory, valid until `model_free`; the length is
//! given by the matching count function. All numbers are little-endian
//! (native); `u32::MAX` means absent in a `u32` column. Ranges are flat
//! `[start_line, start_col, end_line, end_col]` quadruples (0-based, end
//! exclusive, byte columns).
//!
//! Strings are `StrRef { ptr, len }` (UTF-8, not NUL-terminated) pointing
//! into the model; `ptr` is null for an absent string.
//!
//! Counts: entities → `model_entity_count`; relations (and the
//! `model_relation_*` columns) → `model_relation_count`; sites →
//! `model_site_count`; `model_relation_site_offsets` has one more element
//! than relations (CSR); kinds, languages and relation kinds have their own
//! counts. Provenance codes: 0 tree-sitter, 1 SCIP, 2 frontend, 3 derived.
//!
//! The entities of a handle are in id order, so an entity id is its row.
//!
//! # Safety
//!
//! The functions taking a handle require a pointer returned by `model_open`
//! and not yet freed (or null, which yields null/0). The handle is not
//! thread-safe: use it from one thread at a time.

#[cfg(feature = "build")]
mod building;
mod columns;
mod offsets;

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use model_core::{EntityId, Model};

pub use columns::{Columns, StrRef};

/// Version of this C ABI; bumped on any incompatible change.
pub const ABI_VERSION: u32 = 1;

/// A model with its columns. Opaque to C.
pub struct Handle {
    model: Model,
    columns: Columns,
    /// Result of the last `model_entity_props_json` call.
    scratch: Vec<u8>,
    /// Build report (empty for a model opened from a file).
    info: String,
}

impl Handle {
    fn new(model: Model, info: String) -> Handle {
        let columns = Columns::build(&model);
        Handle {
            model,
            columns,
            scratch: Vec::new(),
            info,
        }
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(message: impl std::fmt::Display) {
    let text = message.to_string().replace('\0', " ");
    LAST_ERROR.with(|e| *e.borrow_mut() = CString::new(text).unwrap_or_default());
}

fn load(path: &Path) -> Result<Handle, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let model: Model = serde_json::from_reader(BufReader::new(file))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Handle::new(model, String::new()))
}

/// # Safety
/// `h` is null or a live handle from `model_open`.
unsafe fn handle<'a>(h: *const Handle) -> Option<&'a Handle> {
    // SAFETY: by the caller's contract `h` is null or points to a live Handle.
    unsafe { h.as_ref() }
}

/// Version of the C ABI.
#[unsafe(no_mangle)]
pub extern "C" fn model_abi_version() -> u32 {
    ABI_VERSION
}

/// Opens a model JSON file. Returns null on error (see `model_last_error`).
///
/// # Safety
/// `path` is a NUL-terminated string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_open(path: *const c_char) -> *mut Handle {
    if path.is_null() {
        set_error("null path");
        return std::ptr::null_mut();
    }
    // SAFETY: `path` is non-null and NUL-terminated by the caller's contract.
    let path = unsafe { CStr::from_ptr(path) };
    let Ok(path) = path.to_str() else {
        set_error("path is not UTF-8");
        return std::ptr::null_mut();
    };
    match load(Path::new(path)) {
        Ok(h) => Box::into_raw(Box::new(h)),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// 1 when this library was compiled with the `build` feature (so that
/// `model_build` exists and works), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn model_has_build() -> u32 {
    u32::from(cfg!(feature = "build"))
}

/// Optional UTF-8 argument: null → `None`.
///
/// # Safety
/// `p` is null or a NUL-terminated string.
#[cfg(feature = "build")]
unsafe fn optional_str(p: *const c_char) -> Result<Option<String>, &'static str> {
    if p.is_null() {
        return Ok(None);
    }
    // SAFETY: `p` is non-null and NUL-terminated by the caller's contract.
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map(|s| Some(s.to_owned()))
        .map_err(|_| "argument is not UTF-8")
}

/// Builds the model of the sources under `root` in this process: no
/// `model.json`. Returns a handle like `model_open`'s, or null on error (see
/// `model_last_error`). The call blocks until the build is done.
///
/// - `scip`: non-zero to use SCIP indexes (reused, or run per `reindex`); zero
///   for tree-sitter only.
/// - `reindex`: `0` never, `1` if missing, `2` if stale, `3` always. Anything
///   but `0` may run an indexer, that is the project's build tool.
/// - `languages`: comma-separated language ids, or null/empty for all detected.
/// - `scope`: comma-separated files or directories relative to `root`, or
///   null/empty for everything.
///
/// No cache is read or written. Only available with the `build` feature
/// (`model_has_build`); without it this returns null.
///
/// # Safety
/// `root` is a NUL-terminated string; `languages` and `scope` are each null or
/// NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_build(
    root: *const c_char,
    scip: u32,
    reindex: u32,
    languages: *const c_char,
    scope: *const c_char,
) -> *mut Handle {
    #[cfg(feature = "build")]
    {
        // SAFETY: forwarded from this function's contract.
        let args = unsafe {
            (
                optional_str(root),
                optional_str(languages),
                optional_str(scope),
            )
        };
        let (Ok(Some(root)), Ok(languages), Ok(scope)) = args else {
            set_error("invalid argument (root missing or not UTF-8)");
            return std::ptr::null_mut();
        };
        let Some(reindex) = building::reindex_of(reindex) else {
            set_error("reindex must be 0, 1, 2 or 3");
            return std::ptr::null_mut();
        };
        let spec = building::Spec {
            scip: scip != 0,
            reindex,
            languages: languages.as_deref().and_then(building::list),
            scope: scope.as_deref().and_then(building::list),
        };
        match building::run(Path::new(&root), &spec) {
            Ok((model, info)) => Box::into_raw(Box::new(Handle::new(model, info))),
            Err(e) => {
                set_error(e);
                std::ptr::null_mut()
            }
        }
    }
    #[cfg(not(feature = "build"))]
    {
        let _ = (root, scip, reindex, languages, scope);
        set_error("this library was built without the `build` feature");
        std::ptr::null_mut()
    }
}

/// Report of `model_build` (UTF-8, not NUL-terminated): counts, then one
/// line per warning. Empty for a model opened with `model_open`. Its length
/// goes to `*len`; valid until `model_free`.
///
/// # Safety
/// `h` is null or a live handle; `len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_build_report(h: *const Handle, len: *mut u32) -> *const u8 {
    // SAFETY: forwarded from this function's contract.
    let (h, len) = unsafe { (handle(h), len.as_mut()) };
    let n = h.map_or(0, |h| u32::try_from(h.info.len()).unwrap_or(u32::MAX));
    if let Some(len) = len {
        *len = n;
    }
    h.map_or(std::ptr::null(), |h| h.info.as_ptr())
}

/// Frees a handle; all pointers obtained from it become invalid.
///
/// # Safety
/// `h` is null or a live handle from `model_open`, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_free(h: *mut Handle) {
    if !h.is_null() {
        // SAFETY: `h` came from `Box::into_raw` in `model_open` and is freed once.
        drop(unsafe { Box::from_raw(h) });
    }
}

/// Message of the last error on this thread (NUL-terminated, empty if none).
/// Valid until the next failing call on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn model_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

macro_rules! counts {
    ($($name:ident => |$h:ident| $e:expr),* $(,)?) => {$(
        /// Number of rows of the matching columns (0 for a null handle).
        ///
        /// # Safety
        /// `h` is null or a live handle from `model_open`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(h: *const Handle) -> u64 {
            // SAFETY: forwarded from this function's contract.
            match unsafe { handle(h) } {
                Some($h) => $e as u64,
                None => 0,
            }
        }
    )*};
}

counts! {
    model_entity_count => |h| h.model.entity_count(),
    model_relation_count => |h| h.model.relation_count(),
    model_site_count => |h| h.model.site_count(),
    model_kind_count => |h| h.columns.kind_names.len(),
    model_language_count => |h| h.columns.language_names.len(),
    model_relation_kind_count => |h| h.columns.relation_kind_names.len(),
}

macro_rules! columns {
    ($($name:ident: $ty:ty => $field:ident),* $(,)?) => {$(
        /// Pointer to a column of the handle; null for a null handle.
        ///
        /// # Safety
        /// `h` is null or a live handle from `model_open`; the pointer is
        /// valid until `model_free`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(h: *const Handle) -> *const $ty {
            // SAFETY: forwarded from this function's contract.
            match unsafe { handle(h) } {
                Some(h) => h.columns.$field.as_ptr(),
                None => std::ptr::null(),
            }
        }
    )*};
}

columns! {
    model_entity_keys: StrRef => entity_keys,
    model_entity_names: StrRef => entity_names,
    model_entity_files: StrRef => entity_files,
    model_entity_parents: u32 => entity_parents,
    model_entity_kinds: u32 => entity_kinds,
    model_entity_languages: u32 => entity_languages,
    model_entity_flags: u32 => entity_flags,
    model_entity_ranges: u32 => entity_ranges,
    model_entity_name_ranges: u32 => entity_name_ranges,
    model_relation_from: u32 => relation_from,
    model_relation_to: u32 => relation_to,
    model_relation_kinds: u32 => relation_kinds,
    model_relation_provenances: u32 => relation_provenances,
    model_relation_site_offsets: u32 => site_offsets,
    model_site_ranges: u32 => site_ranges,
    model_site_flags: u8 => site_flags,
    model_kind_names: StrRef => kind_names,
    model_kind_archetypes: StrRef => kind_archetypes,
    model_kind_fm3: StrRef => kind_fm3,
    model_kind_traits: u64 => kind_traits,
    model_language_names: StrRef => language_names,
    model_relation_kind_names: StrRef => relation_kind_names,
}

/// Props of an entity as a JSON object (UTF-8, not NUL-terminated); its
/// length goes to `*len`. Only entities flagged `has props` have any.
/// Returns null (and sets `*len` to 0) for an unknown entity. The buffer is
/// valid until the next call of this function on the same handle.
///
/// # Safety
/// `h` is a live handle from `model_open` (not shared with another thread),
/// `len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_entity_props_json(
    h: *mut Handle,
    entity: u32,
    len: *mut u32,
) -> *const u8 {
    // SAFETY: `len` is null or writable by the caller's contract.
    let set_len = |n: u32| {
        if let Some(len) = unsafe { len.as_mut() } {
            *len = n;
        }
    };
    // SAFETY: `h` is null or a live, unshared handle by the caller's contract.
    let Some(h) = (unsafe { h.as_mut() }) else {
        set_len(0);
        return std::ptr::null();
    };
    let Some(e) = h.model.entity(EntityId::from_index(entity)) else {
        set_len(0);
        return std::ptr::null();
    };
    match serde_json::to_vec(e.props()) {
        Ok(bytes) => {
            h.scratch = bytes;
            set_len(u32::try_from(h.scratch.len()).unwrap_or(u32::MAX));
            h.scratch.as_ptr()
        }
        Err(err) => {
            set_error(err);
            set_len(0);
            std::ptr::null()
        }
    }
}

/// LOC and cyclomatic complexity of an entity, as computed by
/// `model_analysis`. Writes `*loc` and `*cyclomatic`; returns 0, or -1 with
/// the reason in `model_last_error` for a null handle, null output pointer
/// or unknown entity.
///
/// # Safety
/// `h` is null or a live handle from `model_open` (not shared with another
/// thread); `loc` and `cyclomatic` are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_entity_metrics(
    h: *const Handle,
    entity: u32,
    loc: *mut u32,
    cyclomatic: *mut u32,
) -> i32 {
    if loc.is_null() || cyclomatic.is_null() {
        set_error("null argument");
        return -1;
    }
    // SAFETY: `h` is null or a live, unshared handle by the caller's contract.
    let Some(h) = (unsafe { handle(h) }) else {
        set_error("null handle");
        return -1;
    };
    let id = EntityId::from_index(entity);
    let Some(e) = h.model.entity(id) else {
        set_error("unknown entity");
        return -1;
    };
    let lines = model_analysis::lines_of_code(e);
    let cyclo = model_analysis::cyclomatic_complexity(&h.model, id);
    // SAFETY: `loc` and `cyclomatic` are writable by the caller's contract.
    unsafe {
        *loc = lines;
        *cyclomatic = cyclo;
    }
    0
}

/// LOC and cyclomatic complexity for `count` consecutive entities starting at
/// `first`, written to `locs` and `cyclos` (each `count` `u32`, in entity id
/// order). Unknown ids get `0`. Returns 0, or -1 with the reason in
/// `model_last_error` for a null handle or null output pointer.
///
/// # Safety
/// `h` is null or a live handle from `model_open` (not shared with another
/// thread); `locs` and `cyclos` are each null (only when `count` is 0) or
/// writable for `count` `u32`, without overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_entity_metrics_batch(
    h: *const Handle,
    first: u32,
    count: u32,
    locs: *mut u32,
    cyclos: *mut u32,
) -> i32 {
    if count > 0 && (locs.is_null() || cyclos.is_null()) {
        set_error("null argument");
        return -1;
    }
    // SAFETY: `h` is null or a live, unshared handle by the caller's contract.
    let Some(h) = (unsafe { handle(h) }) else {
        set_error("null handle");
        return -1;
    };
    if count == 0 {
        return 0;
    }
    // SAFETY: both buffers are writable for `count` u32 and do not overlap,
    // by the caller's contract (and `count > 0` here).
    let (locs, cyclos) = unsafe {
        (
            std::slice::from_raw_parts_mut(locs, count as usize),
            std::slice::from_raw_parts_mut(cyclos, count as usize),
        )
    };
    for (i, (loc, cyclo)) in locs.iter_mut().zip(cyclos).enumerate() {
        let Some(index) = first.checked_add(u32::try_from(i).unwrap_or(u32::MAX)) else {
            break;
        };
        let id = EntityId::from_index(index);
        if let Some(e) = h.model.entity(id) {
            *loc = model_analysis::lines_of_code(e);
            *cyclo = model_analysis::cyclomatic_complexity(&h.model, id);
        }
    }
    0
}

/// Converts `n` ranges of the source file `path` (flat quadruples as in the
/// columns) to character offsets, as the Moose `IndexedFileAnchor` wants
/// them: `out[2 * i]` is the 1-based offset of the first character,
/// `out[2 * i + 1]` that of the last one (inclusive); a character is a
/// Unicode scalar value. Positions past the end are clamped.
/// Returns 0, or -1 with the reason in `model_last_error`.
///
/// # Safety
/// `path` is NUL-terminated; `ranges` points to `4 * n` readable `u32`;
/// `out` points to `2 * n` writable `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn model_char_offsets(
    path: *const c_char,
    ranges: *const u32,
    n: u32,
    out: *mut u32,
) -> i32 {
    if path.is_null() || (n > 0 && (ranges.is_null() || out.is_null())) {
        set_error("null argument");
        return -1;
    }
    // SAFETY: `path` is non-null and NUL-terminated by the caller's contract.
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        set_error("path is not UTF-8");
        return -1;
    };
    let index = match offsets::FileIndex::load(Path::new(path)) {
        Ok(i) => i,
        Err(e) => {
            set_error(format!("{path}: {e}"));
            return -1;
        }
    };
    let n = n as usize;
    // SAFETY: the caller guarantees `4 * n` readable and `2 * n` writable
    // `u32`, and that the two buffers do not overlap.
    let (ranges, out) = unsafe {
        (
            std::slice::from_raw_parts(ranges, 4 * n),
            std::slice::from_raw_parts_mut(out, 2 * n),
        )
    };
    let (ranges, _) = ranges.as_chunks::<4>();
    let (out, _) = out.as_chunks_mut::<2>();
    for (r, o) in ranges.iter().zip(out.iter_mut()) {
        let (start, end) = index.span(*r);
        *o = [start, end];
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn temp(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("model-ffi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let p = dir.join(name);
        std::fs::write(&p, contents).expect("write");
        p
    }

    fn model_json() -> String {
        use model_core::{EntityKind, ModelBuilder, Provenance, Range, RelationKind};
        let mut b = ModelBuilder::new();
        let a = b.entity("k A", EntityKind::Class, "A");
        let m = b.entity("k m", EntityKind::Method, "m");
        b.set_parent(m, Some(a));
        b.set_prop(a, "x", 1i64);
        b.occurrence(
            &RelationKind::Calls,
            m,
            a,
            Provenance::Scip,
            Range::new(1, 2, 1, 3),
        );
        serde_json::to_string(&b.build()).expect("serialise")
    }

    #[test]
    fn open_read_free() {
        let path = temp("model.json", &model_json());
        let c = CString::new(path.to_str().expect("utf8")).expect("cstring");
        // SAFETY: valid NUL-terminated path; the handle is freed once.
        unsafe {
            let h = model_open(c.as_ptr());
            assert!(!h.is_null());
            assert_eq!(model_entity_count(h), 2);
            assert_eq!(model_relation_count(h), 1);
            assert_eq!(model_site_count(h), 1);
            let parents = std::slice::from_raw_parts(model_entity_parents(h), 2);
            assert_eq!(parents, &[columns::NONE, 0]);
            let mut len = 0;
            let p = model_entity_props_json(h, 0, &mut len);
            let json = std::slice::from_raw_parts(p, len as usize);
            assert_eq!(json, br#"{"x":1}"#);
            assert!(model_entity_props_json(h, 99, &mut len).is_null());
            assert_eq!(len, 0);
            model_free(h);
        }
    }

    fn metrics_json() -> String {
        use model_core::{BRANCHES_PROP, EntityKind, ModelBuilder, Range, Value};
        let mut b = ModelBuilder::new();
        let c = b.entity("k C", EntityKind::Class, "C");
        let m = b.entity("k m", EntityKind::Method, "m");
        b.set_parent(m, Some(c));
        b.set_source(c, "C.java", Range::new(1, 0, 8, 0), None);
        b.set_source(m, "C.java", Range::new(2, 0, 6, 0), None);
        b.set_prop(m, BRANCHES_PROP, Value::Int(2));
        serde_json::to_string(&b.build()).expect("serialise")
    }

    #[test]
    fn entity_metrics_over_the_abi() {
        let path = temp("metrics.json", &metrics_json());
        let c = CString::new(path.to_str().expect("utf8")).expect("cstring");
        // SAFETY: valid path; output pointers are writable; handle freed once.
        unsafe {
            let h = model_open(c.as_ptr());
            assert!(!h.is_null());
            let mut loc = 0;
            let mut cyclo = 0;
            assert_eq!(model_entity_metrics(h, 0, &mut loc, &mut cyclo), 0);
            assert_eq!(loc, 8);
            assert_eq!(cyclo, 3); // the `m` child: 1 + 2 branches
            assert_eq!(model_entity_metrics(h, 1, &mut loc, &mut cyclo), 0);
            assert_eq!(loc, 5);
            assert_eq!(cyclo, 3);
            assert_eq!(model_entity_metrics(h, 99, &mut loc, &mut cyclo), -1);
            let mut locs = [0u32; 2];
            let mut cyclos = [0u32; 2];
            assert_eq!(
                model_entity_metrics_batch(h, 0, 2, locs.as_mut_ptr(), cyclos.as_mut_ptr()),
                0
            );
            assert_eq!(locs, [8, 5]);
            assert_eq!(cyclos, [3, 3]);
            // A zero-length batch accepts null buffers (no slice is built).
            assert_eq!(
                model_entity_metrics_batch(h, 0, 0, std::ptr::null_mut(), std::ptr::null_mut()),
                0
            );
            model_free(h);
        }
    }

    #[test]
    fn open_reports_errors_and_null_is_inert() {
        let c = CString::new("/nonexistent/model.json").expect("cstring");
        // SAFETY: valid path string; null handles are accepted.
        unsafe {
            assert!(model_open(c.as_ptr()).is_null());
            let msg = CStr::from_ptr(model_last_error()).to_string_lossy();
            assert!(msg.contains("nonexistent"), "{msg}");
            assert_eq!(model_entity_count(std::ptr::null()), 0);
            assert!(model_entity_keys(std::ptr::null()).is_null());
            model_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn char_offsets_over_the_abi() {
        let path = temp("A.java", "éa\nb");
        let c = CString::new(path.to_str().expect("utf8")).expect("cstring");
        let ranges = [0u32, 0, 0, 2, 1, 0, 1, 1];
        let mut out = [0u32; 4];
        // SAFETY: buffers sized as documented.
        let rc = unsafe { model_char_offsets(c.as_ptr(), ranges.as_ptr(), 2, out.as_mut_ptr()) };
        assert_eq!(rc, 0);
        assert_eq!(out, [1, 1, 4, 4]);
    }

    #[test]
    fn char_offsets_missing_file_is_an_error() {
        let c = CString::new("/nonexistent/A.java").expect("cstring");
        let mut out = [0u32; 2];
        let r = [0u32; 4];
        // SAFETY: buffers sized as documented.
        let rc = unsafe { model_char_offsets(c.as_ptr(), r.as_ptr(), 1, out.as_mut_ptr()) };
        assert_eq!(rc, -1);
    }

    #[cfg(feature = "build")]
    fn java_project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("model-ffi-{}-{name}", std::process::id()));
        let src = dir.join("src/acme");
        std::fs::create_dir_all(&src).expect("project dir");
        std::fs::write(
            src.join("A.java"),
            "package acme;\n\npublic class A {\n    public void run(B b) { b.go(); }\n}\n",
        )
        .expect("A.java");
        std::fs::write(
            src.join("B.java"),
            "package acme;\n\npublic class B {\n    public void go() {}\n}\n",
        )
        .expect("B.java");
        dir
    }

    #[cfg(feature = "build")]
    #[test]
    fn build_from_sources_without_json() {
        let root = java_project("build");
        let c = CString::new(root.to_str().expect("utf8")).expect("cstring");
        // SAFETY: valid NUL-terminated root, null optional arguments; the
        // handle is freed once.
        unsafe {
            assert_eq!(model_has_build(), 1);
            let h = model_build(c.as_ptr(), 0, 0, std::ptr::null(), std::ptr::null());
            assert!(!h.is_null(), "{:?}", CStr::from_ptr(model_last_error()));
            assert!(model_entity_count(h) >= 6);
            assert!(model_relation_count(h) >= 1);
            let mut len = 0;
            let p = model_build_report(h, &mut len);
            let report =
                std::str::from_utf8(std::slice::from_raw_parts(p, len as usize)).expect("utf8");
            assert!(report.contains("entities"), "{report}");
            model_free(h);
        }
    }

    #[cfg(feature = "build")]
    #[test]
    fn build_honours_languages_and_rejects_bad_arguments() {
        let root = java_project("filter");
        let c = CString::new(root.to_str().expect("utf8")).expect("cstring");
        let python = CString::new("python").expect("cstring");
        // SAFETY: valid NUL-terminated strings; handles freed once.
        unsafe {
            let h = model_build(c.as_ptr(), 0, 0, python.as_ptr(), std::ptr::null());
            assert!(!h.is_null());
            assert_eq!(model_entity_count(h), 0);
            model_free(h);
            assert!(model_build(c.as_ptr(), 0, 7, std::ptr::null(), std::ptr::null()).is_null());
            assert!(
                model_build(std::ptr::null(), 0, 0, std::ptr::null(), std::ptr::null()).is_null()
            );
            let missing = CString::new("/nonexistent/project").expect("cstring");
            assert!(
                model_build(missing.as_ptr(), 0, 0, std::ptr::null(), std::ptr::null()).is_null()
            );
        }
    }

    #[cfg(not(feature = "build"))]
    #[test]
    fn build_is_reported_unavailable_without_the_feature() {
        let c = CString::new("/tmp").expect("cstring");
        // SAFETY: valid NUL-terminated root.
        unsafe {
            assert_eq!(model_has_build(), 0);
            assert!(model_build(c.as_ptr(), 0, 0, std::ptr::null(), std::ptr::null()).is_null());
            let msg = CStr::from_ptr(model_last_error()).to_string_lossy();
            assert!(msg.contains("build"), "{msg}");
        }
    }
}
