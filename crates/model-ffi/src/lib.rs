//! C ABI over a frozen [`Model`](model_core::Model), for foreign importers
//! (Pharo through UFFI, ...).
//!
//! Depends on `model-core` only: it exposes raw columns and tables, with no
//! knowledge of any language or of Famix. Mapping to a target metamodel
//! (FM3 class per kind, containers, associations) is the client's business,
//! driven by [`KindInfo`](model_core::KindInfo) (`kind_fm3`).
//!
//! # Usage
//!
//! `model_open` loads a model JSON and returns an opaque handle (null on
//! error, see `model_last_error`). Every `model_*` column getter returns a
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
    let columns = Columns::build(&model);
    Ok(Handle {
        model,
        columns,
        scratch: Vec::new(),
    })
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
}
