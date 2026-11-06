//! Just enough JPQL to find entity references, identification variables and
//! path expressions in a query string. Offsets are bytes of the query.

/// A word of a query: an identifier or a path (`u.address.city`), or one punctuation character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word<'a> {
    pub at: usize,
    pub s: &'a str,
}

impl Word<'_> {
    pub fn is(&self, keyword: &str) -> bool {
        self.s.eq_ignore_ascii_case(keyword)
    }

    pub fn is_name(&self) -> bool {
        self.s.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
    }
}

/// Words that can't be an identification variable.
const NOT_ALIASES: [&str; 18] = [
    "where", "join", "left", "right", "inner", "outer", "fetch", "on", "order", "group", "having", "set", "union", "with",
    "as", "select", "from", "and",
];

fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

/// The words of `q`; string literals, `:name`/`?1` parameters and SpEL `#{…}` skipped.
pub fn words(q: &str) -> Vec<Word<'_>> {
    let b = q.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'\'' || c == b'"' {
            i += 1 + b[i + 1..].iter().position(|&x| x == c).map_or(b.len() - i - 1, |p| p + 1);
        } else if c == b'#' && b.get(i + 1) == Some(&b'{') {
            i += b[i..].iter().position(|&x| x == b'}').map_or(b.len() - i, |p| p + 1);
        } else if c == b':' || c == b'?' {
            i += 1;
            while i < b.len() && ident(b[i]) {
                i += 1;
            }
        } else if ident(c) {
            let start = i;
            while i < b.len() && (ident(b[i]) || b[i] == b'.') {
                i += 1;
            }
            out.push(Word { at: start, s: &q[start..i] });
        } else {
            let start = i;
            i += 1;
            while !q.is_char_boundary(i) {
                i += 1;
            }
            out.push(Word { at: start, s: &q[start..i] });
        }
    }
    out
}

/// A range variable declaration: `FROM User u` (an entity name) or `JOIN u.orders o` (a path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decl<'a> {
    pub target: Word<'a>,
    pub alias: Option<&'a str>,
}

impl Decl<'_> {
    pub fn is_path(&self) -> bool {
        self.target.s.contains('.')
    }
}

/// Whether the `from` at `i` belongs to a function call (`extract(year from d)`): the innermost
/// unclosed `(` has no `select` between it and the `from`.
fn function_from(words: &[Word], i: usize) -> bool {
    let mut closed = 0;
    for w in words[..i].iter().rev() {
        match w.s {
            ")" => closed += 1,
            "(" if closed > 0 => closed -= 1,
            "(" => return true,
            _ if closed == 0 && w.is("select") => return false,
            _ => {}
        }
    }
    false
}

/// The declarations after `FROM`, `JOIN` (`JOIN FETCH`) and `UPDATE`, in order.
pub fn declarations<'a>(words: &[Word<'a>]) -> Vec<Decl<'a>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if !(w.is("from") || w.is("join") || w.is("update")) || (w.is("from") && function_from(words, i)) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if words.get(j).is_some_and(|f| f.is("fetch")) {
            j += 1;
        }
        while let Some(target) = words.get(j).filter(|t| t.is_name()) {
            j += 1;
            if words.get(j).is_some_and(|a| a.is("as")) {
                j += 1;
            }
            let alias = words.get(j).filter(|a| a.is_name() && !a.s.contains('.') && !NOT_ALIASES.iter().any(|k| a.is(k)));
            if alias.is_some() {
                j += 1;
            }
            out.push(Decl { target: *target, alias: alias.map(|a| a.s) });
            // `FROM User u, Order o`
            if !(w.is("from") && words.get(j).is_some_and(|c| c.s == ",")) {
                break;
            }
            j += 1;
        }
        i = j.max(i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_declarations_and_paths() {
        let q = "select distinct u from User u left join fetch u.orders as o, IN(x) where o.total > :min and u.name like '%from X%' or #{#entityName} = ?1";
        let ws = words(q);
        assert!(ws.iter().all(|w| w.s != "min" && w.s != "X" && w.s != "entityName" && w.s != "1"));
        let decls: Vec<_> = declarations(&ws).into_iter().map(|d| (d.target.s, d.alias)).collect();
        assert_eq!(decls, [("User", Some("u")), ("u.orders", Some("o"))]);
        let q = "SELECT a FROM Author a, Book AS b WHERE EXTRACT(YEAR FROM b.date) = 1; UPDATE Book SET title = 'x'";
        let decls: Vec<_> = declarations(&words(q)).into_iter().map(|d| (d.target.s, d.alias)).collect();
        assert_eq!(decls, [("Author", Some("a")), ("Book", Some("b")), ("Book", None)]);
    }
}
