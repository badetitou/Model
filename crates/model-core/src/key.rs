//! SCIP-like entity keys.
//!
//! Keys follow the [SCIP symbol grammar](https://github.com/sourcegraph/scip/blob/main/scip.proto)
//! — `<scheme> <manager> <package> <version> <descriptors>` — so an entity
//! keyed by a SCIP indexer and one keyed by a tree-sitter frontend look
//! alike and can be matched on their descriptors. Frontends without an
//! indexer use the scheme [`SCHEME`] and the language as manager:
//! `model java . . com/acme/A#run().`

use std::fmt;

use crate::entity::{EntityKind, LanguageId};

/// Scheme of keys built by Model frontends (not by a SCIP indexer).
pub const SCHEME: &str = "model";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Descriptor {
    /// `name/` — package, namespace, module, file.
    Namespace(String),
    /// `name#` — class, interface, trait, enum, struct.
    Type(String),
    /// `name.` — field, property, constant, variable.
    Term(String),
    /// `name(disambiguator).` — method, constructor, function.
    Method { name: String, disambiguator: String },
    /// `[name]`
    TypeParameter(String),
    /// `(name)`
    Parameter(String),
    /// `name:`
    Meta(String),
    /// `name!`
    Macro(String),
}

impl Descriptor {
    /// Descriptor SCIP uses for an entity of `kind`.
    pub fn for_kind(kind: EntityKind, name: &str) -> Descriptor {
        let name = name.to_string();
        match kind {
            EntityKind::Namespace | EntityKind::File => Descriptor::Namespace(name),
            k if k.is_type() => Descriptor::Type(name),
            k if k.is_callable() => Descriptor::Method {
                name,
                disambiguator: String::new(),
            },
            _ => Descriptor::Term(name),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Descriptor::Namespace(n)
            | Descriptor::Type(n)
            | Descriptor::Term(n)
            | Descriptor::TypeParameter(n)
            | Descriptor::Parameter(n)
            | Descriptor::Meta(n)
            | Descriptor::Macro(n) => n,
            Descriptor::Method { name, .. } => name,
        }
    }
}

/// `manager name version`; empty parts are written `.`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Package {
    pub manager: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SymbolKey {
    Global {
        scheme: String,
        package: Package,
        descriptors: Vec<Descriptor>,
    },
    /// `local <id>`: only unique within one file.
    Local(String),
}

impl SymbolKey {
    /// Root key of a Model frontend for `language`: `model <language> . . `.
    pub fn new(language: &LanguageId) -> SymbolKey {
        SymbolKey::Global {
            scheme: SCHEME.to_string(),
            package: Package {
                manager: language.as_str().to_string(),
                ..Package::default()
            },
            descriptors: Vec::new(),
        }
    }

    /// This key extended with `d` (a local key is returned unchanged).
    pub fn child(&self, d: Descriptor) -> SymbolKey {
        let mut k = self.clone();
        if let SymbolKey::Global { descriptors, .. } = &mut k {
            descriptors.push(d);
        }
        k
    }

    /// Key of a source file: one namespace per path segment, as
    /// scip-typescript names modules (`src/`a.ts`/`).
    pub fn for_file(language: &LanguageId, path: &str) -> SymbolKey {
        path.split('/')
            .filter(|s| !s.is_empty())
            .fold(SymbolKey::new(language), |k, s| {
                k.child(Descriptor::Namespace(s.to_string()))
            })
    }

    pub fn descriptors(&self) -> &[Descriptor] {
        match self {
            SymbolKey::Global { descriptors, .. } => descriptors,
            SymbolKey::Local(_) => &[],
        }
    }

    /// Same descriptors, whatever the scheme and package: how a key built
    /// from syntax meets the key a SCIP indexer gave the same symbol.
    pub fn same_descriptors(&self, other: &SymbolKey) -> bool {
        matches!(
            (self, other),
            (SymbolKey::Global { .. }, SymbolKey::Global { .. })
        ) && self.descriptors() == other.descriptors()
    }

    pub fn parse(s: &str) -> Result<SymbolKey, KeyError> {
        Parser {
            chars: s.chars().collect(),
            pos: 0,
        }
        .key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyError {
    pub position: usize,
    pub message: String,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid key at {}: {}", self.position, self.message)
    }
}

impl std::error::Error for KeyError {}

fn is_simple(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '$')
}

struct Ident<'a>(&'a str);

impl fmt::Display for Ident<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.0.is_empty() && self.0.chars().all(is_simple) {
            f.write_str(self.0)
        } else {
            write!(f, "`{}`", self.0.replace('`', "``"))
        }
    }
}

struct Field<'a>(&'a str);

impl fmt::Display for Field<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str(".")
        } else {
            f.write_str(&self.0.replace(' ', "  "))
        }
    }
}

impl fmt::Display for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Descriptor::Namespace(n) => write!(f, "{}/", Ident(n)),
            Descriptor::Type(n) => write!(f, "{}#", Ident(n)),
            Descriptor::Term(n) => write!(f, "{}.", Ident(n)),
            Descriptor::Meta(n) => write!(f, "{}:", Ident(n)),
            Descriptor::Macro(n) => write!(f, "{}!", Ident(n)),
            Descriptor::TypeParameter(n) => write!(f, "[{}]", Ident(n)),
            Descriptor::Parameter(n) => write!(f, "({})", Ident(n)),
            Descriptor::Method {
                name,
                disambiguator,
            } if disambiguator.is_empty() => write!(f, "{}().", Ident(name)),
            Descriptor::Method {
                name,
                disambiguator,
            } => write!(f, "{}({}).", Ident(name), Ident(disambiguator)),
        }
    }
}

impl fmt::Display for SymbolKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolKey::Local(id) => write!(f, "local {id}"),
            SymbolKey::Global {
                scheme,
                package,
                descriptors,
            } => {
                write!(
                    f,
                    "{} {} {} {} ",
                    Field(scheme),
                    Field(&package.manager),
                    Field(&package.name),
                    Field(&package.version)
                )?;
                descriptors.iter().try_for_each(|d| write!(f, "{d}"))
            }
        }
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn err<T>(&self, message: &str) -> Result<T, KeyError> {
        Err(KeyError {
            position: self.pos,
            message: message.to_string(),
        })
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn expect(&mut self, c: char) -> Result<(), KeyError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            self.err(&format!("expected `{c}`"))
        }
    }

    fn key(mut self) -> Result<SymbolKey, KeyError> {
        let text: String = self.chars.iter().collect();
        if let Some(id) = text.strip_prefix("local ") {
            return Ok(SymbolKey::Local(id.to_string()));
        }
        let scheme = self.field()?;
        if scheme.is_empty() {
            return self.err("empty scheme");
        }
        let package = Package {
            manager: self.field()?,
            name: self.field()?,
            version: self.field()?,
        };
        let mut descriptors = Vec::new();
        while self.peek().is_some() {
            descriptors.push(self.descriptor()?);
        }
        Ok(SymbolKey::Global {
            scheme,
            package,
            descriptors,
        })
    }

    /// Space-terminated; `  ` is an escaped space, `.` alone is empty.
    fn field(&mut self) -> Result<String, KeyError> {
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return self.err("unterminated key header"),
                Some(' ') if self.chars.get(self.pos + 1) == Some(&' ') => {
                    s.push(' ');
                    self.pos += 2;
                }
                Some(' ') => {
                    self.pos += 1;
                    return Ok(if s == "." { String::new() } else { s });
                }
                Some(c) => {
                    s.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn name(&mut self) -> Result<String, KeyError> {
        if self.peek() == Some('`') {
            self.pos += 1;
            let mut s = String::new();
            loop {
                match self.peek() {
                    None => return self.err("unterminated `name`"),
                    Some('`') if self.chars.get(self.pos + 1) == Some(&'`') => {
                        s.push('`');
                        self.pos += 2;
                    }
                    Some('`') => {
                        self.pos += 1;
                        return Ok(s);
                    }
                    Some(c) => {
                        s.push(c);
                        self.pos += 1;
                    }
                }
            }
        }
        let start = self.pos;
        while self.peek().is_some_and(is_simple) {
            self.pos += 1;
        }
        if start == self.pos {
            return self.err("expected a name");
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn descriptor(&mut self) -> Result<Descriptor, KeyError> {
        match self.peek() {
            Some('[') => {
                self.pos += 1;
                let n = self.name()?;
                self.expect(']')?;
                return Ok(Descriptor::TypeParameter(n));
            }
            Some('(') => {
                self.pos += 1;
                let n = self.name()?;
                self.expect(')')?;
                return Ok(Descriptor::Parameter(n));
            }
            _ => {}
        }
        let n = self.name()?;
        let suffix = self.peek();
        self.pos += 1;
        Ok(match suffix {
            Some('/') => Descriptor::Namespace(n),
            Some('#') => Descriptor::Type(n),
            Some('.') => Descriptor::Term(n),
            Some(':') => Descriptor::Meta(n),
            Some('!') => Descriptor::Macro(n),
            Some('(') => {
                let disambiguator = if self.peek() == Some(')') {
                    String::new()
                } else {
                    self.name()?
                };
                self.expect(')')?;
                self.expect('.')?;
                Descriptor::Method {
                    name: n,
                    disambiguator,
                }
            }
            _ => {
                self.pos -= 1;
                return self.err("expected a descriptor suffix (/ # . : ! or `(`)");
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: &str) -> SymbolKey {
        let k = SymbolKey::parse(s).unwrap();
        assert_eq!(k.to_string(), s);
        k
    }

    #[test]
    fn builds_frontend_keys() {
        let java = LanguageId::new("java");
        let a = SymbolKey::new(&java)
            .child(Descriptor::Namespace("com".into()))
            .child(Descriptor::Namespace("acme".into()))
            .child(Descriptor::for_kind(EntityKind::Class, "A"));
        let run = a.child(Descriptor::for_kind(EntityKind::Method, "run"));
        assert_eq!(run.to_string(), "model java . . com/acme/A#run().");
        assert_eq!(
            a.child(Descriptor::for_kind(EntityKind::Field, "x"))
                .to_string(),
            "model java . . com/acme/A#x."
        );
    }

    #[test]
    fn parses_indexer_symbols() {
        let k = round_trip("semanticdb maven maven/org.acme/app 1.0 com/acme/A#run(+1).");
        let SymbolKey::Global { package, .. } = &k else {
            panic!("global expected")
        };
        assert_eq!(package.name, "maven/org.acme/app");
        assert_eq!(
            k.descriptors().last(),
            Some(&Descriptor::Method {
                name: "run".into(),
                disambiguator: "+1".into()
            })
        );
        round_trip("scip-typescript npm @acme/web 2.0.0 src/`app.ts`/App#render().");
        round_trip("rust-analyzer cargo std . vec/Vec#[T]new().(capacity)");
        round_trip("local 42");
    }

    #[test]
    fn escapes_names_and_fields() {
        let k = SymbolKey::Global {
            scheme: "model".into(),
            package: Package {
                manager: "pharo".into(),
                name: "My Project".into(),
                version: String::new(),
            },
            descriptors: vec![
                Descriptor::Namespace("src/A.java".into()),
                Descriptor::Method {
                    name: "at:put:".into(),
                    disambiguator: String::new(),
                },
                Descriptor::Term("a`b".into()),
            ],
        };
        let s = k.to_string();
        assert_eq!(
            s,
            "model pharo My  Project . `src/A.java`/`at:put:`().`a``b`."
        );
        assert_eq!(SymbolKey::parse(&s).unwrap(), k);
    }

    #[test]
    fn file_keys_have_one_namespace_per_segment() {
        let k = SymbolKey::for_file(&LanguageId::new("typescript"), "src/app/a.service.ts");
        assert_eq!(
            k.to_string(),
            "model typescript . . src/app/`a.service.ts`/"
        );
        let scip = SymbolKey::parse("scip-typescript npm app 1.0 src/app/`a.service.ts`/").unwrap();
        assert!(k.same_descriptors(&scip));
    }

    #[test]
    fn matches_on_descriptors_across_schemes() {
        let ts = SymbolKey::parse("model java . . com/acme/A#run().").unwrap();
        let scip = SymbolKey::parse("semanticdb maven acme 1.0 com/acme/A#run().").unwrap();
        assert!(ts.same_descriptors(&scip));
        let other = SymbolKey::parse("semanticdb maven acme 1.0 com/acme/A#stop().").unwrap();
        assert!(!ts.same_descriptors(&other));
    }

    #[test]
    fn rejects_malformed_keys() {
        for bad in [
            "",
            "model java",
            "model java . . A",
            "model java . . `A#",
            "model java . . A(.",
        ] {
            assert!(SymbolKey::parse(bad).is_err(), "{bad:?}");
        }
    }
}
