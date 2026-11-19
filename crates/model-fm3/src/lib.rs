//! Reads an FM3 metamodel exported by Moose as JSON (`FMJSONPrinter`).
//!
//! The export is an array of objects, each starting with `"FM3"` (the kind,
//! e.g. `"FM3.Class"`) then `"id"`. References to other objects are written
//! `{"ref": n}`; built-in types (`String`, `Number`, `Boolean`, `Object`, ...)
//! are references by name, `{"ref": "String"}`. An entity nested where a
//! reference is expected is accepted and hoisted into the flat list.
//!
//! ```
//! let json = r#"[
//!   {"FM3":"FM3.Package","id":1,"name":"P","classes":[{"ref":2}]},
//!   {"FM3":"FM3.Class","id":2,"name":"C","package":{"ref":1},"properties":[{"ref":3}]},
//!   {"FM3":"FM3.Property","id":3,"name":"n","class":{"ref":2},"type":{"ref":"String"},
//!    "container":false,"derived":false,"multivalued":false}
//! ]"#;
//! let mm = model_fm3::Metamodel::from_json_str(json).unwrap();
//! assert_eq!(mm.class_by_name("C").unwrap().properties.len(), 1);
//! ```

mod error;
mod metamodel;
mod parse;

pub use error::Error;
pub use metamodel::{Class, Id, Metamodel, Package, Property, Ref, Trait};
