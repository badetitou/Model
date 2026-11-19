use std::fmt;

/// Failure while reading or validating an FM3 export.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The top level is not an array of objects.
    NotAnArray,
    /// Two objects share an id.
    DuplicateId(u64),
    /// A reference to an id that no object in the export has.
    DanglingRef {
        from: u64,
        field: &'static str,
        target: u64,
    },
    /// A field that must be an id (not a built-in name) holds a name.
    NotAnId {
        from: u64,
        field: &'static str,
        name: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "cannot read FM3 export: {e}"),
            Error::Json(e) => write!(f, "invalid FM3 JSON: {e}"),
            Error::NotAnArray => write!(f, "FM3 export must be a JSON array of objects"),
            Error::DuplicateId(id) => write!(f, "duplicate id {id}"),
            Error::DanglingRef {
                from,
                field,
                target,
            } => write!(f, "object {from}: `{field}` refers to unknown id {target}"),
            Error::NotAnId { from, field, name } => {
                write!(f, "object {from}: `{field}` must be an id, found `{name}`")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
