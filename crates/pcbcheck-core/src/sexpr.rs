//! Minimal S-expression reader for KiCad files (.kicad_pcb, .kicad_sch, .kicad_mod).
//! Every node keeps its byte span in the source so fixes can make surgical text edits.

use anyhow::{bail, Result};

/// Byte range `[start, end)` in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sexp {
    Atom(String, Span),
    List(Vec<Sexp>, Span),
}

impl Sexp {
    pub fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(v, _) => v,
            Sexp::Atom(..) => &[],
        }
    }

    pub fn atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(s, _) => Some(s),
            Sexp::List(..) => None,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Sexp::Atom(_, s) | Sexp::List(_, s) => *s,
        }
    }

    /// First element of a list, e.g. `footprint` for `(footprint ...)`.
    pub fn head(&self) -> Option<&str> {
        self.items().first().and_then(Sexp::atom)
    }

    pub fn arg(&self, i: usize) -> Option<&str> {
        self.items().get(i).and_then(Sexp::atom)
    }

    pub fn num(&self, i: usize) -> Option<f64> {
        self.arg(i).and_then(|s| s.parse().ok())
    }

    pub fn children<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items().iter().skip(1).filter(move |c| c.head().is_some_and(|h| h == key))
    }

    pub fn child(&self, key: &str) -> Option<&Sexp> {
        self.items().iter().skip(1).find(|c| c.head().is_some_and(|h| h == key))
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.child(key).and_then(|c| c.arg(1))
    }

    pub fn value_f64(&self, key: &str) -> Option<f64> {
        self.child(key).and_then(|c| c.num(1))
    }

    /// `(at x y)` / `(start x y)` style point.
    pub fn xy(&self) -> (f64, f64) {
        (self.num(1).unwrap_or(0.0), self.num(2).unwrap_or(0.0))
    }

    pub fn child_xy(&self, key: &str) -> (f64, f64) {
        self.child(key).map(Sexp::xy).unwrap_or((0.0, 0.0))
    }
}

pub fn parse(text: &str) -> Result<Sexp> {
    let bytes = text.as_bytes();
    // Each open list: (items, start offset).
    let mut stack: Vec<(Vec<Sexp>, usize)> = vec![(Vec::new(), 0)];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                stack.push((Vec::new(), i));
                i += 1;
            }
            b')' => {
                if stack.len() < 2 {
                    bail!("unbalanced ')' at byte {i}");
                }
                let (list, start) = stack.pop().unwrap();
                stack.last_mut().unwrap().0.push(Sexp::List(list, Span { start, end: i + 1 }));
                i += 1;
            }
            b'"' => {
                let quote = i;
                let mut s = String::new();
                i += 1;
                let mut run = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        s.push_str(&text[run..i]);
                        s.push(match bytes[i + 1] {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            _ => {
                                // Escaped char may be multi-byte; copy it whole.
                                let ch = text[i + 1..].chars().next().unwrap();
                                i += ch.len_utf8() - 1;
                                ch
                            }
                        });
                        i += 2;
                        run = i;
                    } else {
                        i += 1;
                    }
                }
                if i >= bytes.len() {
                    bail!("unterminated string starting at byte {quote}");
                }
                s.push_str(&text[run..i]);
                i += 1;
                stack.last_mut().unwrap().0.push(Sexp::Atom(s, Span { start: quote, end: i }));
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b'(' | b')' | b'"') && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                stack.last_mut().unwrap().0.push(Sexp::Atom(text[start..i].to_string(), Span { start, end: i }));
            }
        }
    }
    if stack.len() != 1 {
        bail!("unbalanced '(' in S-expression");
    }
    match stack.pop().unwrap().0.into_iter().next() {
        Some(root) => Ok(root),
        None => bail!("empty S-expression"),
    }
}

/// Quote a string for KiCad S-expressions.
pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_lists_and_strings() {
        let s = parse(r#"(kicad_pcb (version 20240108) (net 1 "ROW \"0\"") (at 1.5 -2))"#).unwrap();
        assert_eq!(s.head(), Some("kicad_pcb"));
        assert_eq!(s.value("version"), Some("20240108"));
        assert_eq!(s.child("net").unwrap().arg(2), Some("ROW \"0\""));
        assert_eq!(s.child_xy("at"), (1.5, -2.0));
    }

    #[test]
    fn records_spans() {
        let text = r#"(a (width 0.1) (net "x y"))"#;
        let s = parse(text).unwrap();
        let w = s.child("width").unwrap();
        assert_eq!(&text[w.span().start..w.span().end], "(width 0.1)");
        assert_eq!(&text[w.items()[1].span().start..w.items()[1].span().end], "0.1");
        let n = s.child("net").unwrap().items()[1].span();
        assert_eq!(&text[n.start..n.end], "\"x y\"");
        assert_eq!(s.span(), Span { start: 0, end: text.len() });
    }

    #[test]
    fn rejects_unbalanced_input() {
        assert!(parse("(a (b)").is_err());
        assert!(parse("(a))").is_err());
    }
}
