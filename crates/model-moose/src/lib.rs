//! Imports Moose/Famix JSON models into Model and compares them with a
//! model built from the sources: the differential oracle against
//! VerveineJ. Moose export is a separate concern (not here).
//!
//! ```
//! use model_core::ModelBuilder;
//! use model_moose::{ImportOptions, import_json};
//!
//! let json = br#"[
//!   {"FM3":"Famix-Java-Entities.Package","id":1,"name":"p"},
//!   {"FM3":"Famix-Java-Entities.Class","id":2,"name":"A","typeContainer":{"ref":1}}
//! ]"#;
//! let mut b = ModelBuilder::new();
//! let report = import_json(&mut b, json, &ImportOptions::default());
//! assert_eq!(report.entity_count(), 2);
//! let model = b.build();
//! assert_eq!(model.by_key("moose:2").unwrap().parent().unwrap().name(), "p");
//! ```

mod diff;
mod import;
pub mod json;
pub mod mapping;

pub use diff::{DiffReport, DiffRow, diff};
pub use import::{ImportOptions, ImportReport, import_json, import_path};
