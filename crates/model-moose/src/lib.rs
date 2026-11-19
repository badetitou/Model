//! Imports Moose/Famix JSON models into Model and compares them with a
//! model built from the sources: the differential oracle against
//! VerveineJ. It also exports a model as Moose JSON ([`export_json`]).
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
mod export;
mod import;
pub mod json;
pub mod mapping;

pub use diff::{DiffReport, DiffRow, diff};
pub use export::{ExportOptions, ExportReport, export_json, export_path};
pub use import::{ImportOptions, ImportReport, import_json, import_path};
