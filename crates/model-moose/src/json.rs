//! Tolerant streaming reader for Moose JSON: a top-level array of objects,
//! read one object at a time. It accepts what Moose writes and strict
//! parsers reject (raw control characters inside strings, a truncated file:
//! entities read before the error are still delivered).

use std::fmt;

/// A value of an entity property.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// `{"ref": 12}`.
    Ref(u64),
    /// `{"ref": "String"}`: a built-in type.
    Builtin(String),
    List(Vec<Node>),
    /// An entity written inline.
    Entity(Box<RawEntity>),
    /// Any other object.
    Object(Vec<(String, Node)>),
}

/// An entity as written: FM3 class name, optional id, properties in file
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct RawEntity {
    pub class: String,
    pub id: Option<u64>,
    pub props: Vec<(String, Node)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset in the input.
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

/// Calls `each` with every top-level entity of `input`. Stops at the first
/// error, after having delivered everything before it; the error is
/// returned.
pub fn for_each_entity(input: &[u8], mut each: impl FnMut(RawEntity)) -> Result<(), ParseError> {
    let mut p = Parser { s: input, i: 0 };
    p.ws();
    p.expect(b'[')?;
    loop {
        p.ws();
        match p.peek() {
            Some(b']') => return Ok(()),
            Some(b',') => {
                p.i += 1;
                continue;
            }
            None => return Err(p.err("unterminated array")),
            _ => {}
        }
        match p.value()? {
            Node::Entity(e) => each(*e),
            _ => return Err(p.err("top-level item is not an entity")),
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> ParseError {
        ParseError {
            offset: self.i,
            message: message.to_string(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected `{}`", c as char)))
        }
    }

    fn literal(&mut self, word: &str, node: Node) -> Result<Node, ParseError> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(node)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn value(&mut self) -> Result<Node, ParseError> {
        self.ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    match self.peek() {
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Node::List(items));
                        }
                        Some(b',') => self.i += 1,
                        None => return Err(self.err("unterminated list")),
                        _ => items.push(self.value()?),
                    }
                }
            }
            Some(b'"') => Ok(Node::Str(self.string()?)),
            Some(b't') => self.literal("true", Node::Bool(true)),
            Some(b'f') => self.literal("false", Node::Bool(false)),
            Some(b'n') => self.literal("null", Node::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn number(&mut self) -> Result<Node, ParseError> {
        let start = self.i;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        if let Ok(n) = text.parse::<i64>() {
            Ok(Node::Int(n))
        } else if let Ok(f) = text.parse::<f64>() {
            Ok(Node::Float(f))
        } else {
            Err(self.err("invalid number"))
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("unterminated string"));
            };
            self.i += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("unterminated string"));
                    };
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000
                                    + ((cp - 0xD800) << 10)
                                    + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                // Raw control characters are accepted as-is.
                _ => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let digits = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok());
        match digits {
            Some(v) => {
                self.i += 4;
                Ok(v)
            }
            None => Err(self.err("invalid \\u escape")),
        }
    }

    fn object(&mut self) -> Result<Node, ParseError> {
        self.expect(b'{')?;
        let mut props: Vec<(String, Node)> = Vec::new();
        loop {
            self.ws();
            match self.peek() {
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                Some(b',') => {
                    self.i += 1;
                    continue;
                }
                Some(b'"') => {}
                None => return Err(self.err("unterminated object")),
                _ => return Err(self.err("expected a property name")),
            }
            let key = self.string()?;
            self.ws();
            self.expect(b':')?;
            let v = self.value()?;
            props.push((key, v));
        }
        Ok(classify(props))
    }
}

fn classify(mut props: Vec<(String, Node)>) -> Node {
    if let Some((k, Node::Str(class))) = props.first()
        && k == "FM3"
    {
        let class = class.clone();
        props.remove(0);
        let id = match props.first() {
            Some((k, Node::Int(n))) if k == "id" && *n >= 0 => Some(*n as u64),
            _ => None,
        };
        if id.is_some() {
            props.remove(0);
        }
        return Node::Entity(Box::new(RawEntity { class, id, props }));
    }
    if props.len() == 1 && props[0].0 == "ref" {
        match &props[0].1 {
            Node::Int(n) if *n >= 0 => return Node::Ref(*n as u64),
            Node::Str(s) => return Node::Builtin(s.clone()),
            _ => {}
        }
    }
    Node::Object(props)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> (Vec<RawEntity>, Result<(), ParseError>) {
        let mut v = Vec::new();
        let r = for_each_entity(s.as_bytes(), |e| v.push(e));
        (v, r)
    }

    #[test]
    fn entities_refs_and_nesting() {
        let (v, r) = read(
            r#"[{"FM3":"A.B","id":1,"n":"x","k":2,"f":1.5,"b":true,
            "r":{"ref":7},"t":{"ref":"String"},"l":[{"ref":1},{"ref":2}],
            "inner":{"FM3":"A.C","id":9}}]"#,
        );
        assert!(r.is_ok());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].class, "A.B");
        assert_eq!(v[0].id, Some(1));
        assert_eq!(v[0].props[4].1, Node::Ref(7));
        assert_eq!(v[0].props[5].1, Node::Builtin("String".into()));
        assert!(matches!(&v[0].props[7].1, Node::Entity(e) if e.id == Some(9)));
    }

    #[test]
    fn raw_control_characters_and_escapes() {
        let (v, r) = read("[{\"FM3\":\"A.B\",\"s\":\"a\nb\\u00e9\\ud83d\\ude00\"}]");
        assert!(r.is_ok());
        assert_eq!(v[0].props[0].1, Node::Str("a\nbé😀".into()));
    }

    #[test]
    fn truncation_keeps_earlier_entities() {
        let (v, r) = read(r#"[{"FM3":"A.B","id":1},{"FM3":"A.B","id":2,"name":"unfin"#);
        assert_eq!(v.len(), 1);
        assert!(r.is_err());
    }
}
