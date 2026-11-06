//! Field names of a MongoDB `@Query` document, read from its top level only:
//! operators (`$or`) and nested documents are not followed.

/// `(from, to, name)` of the keys of the outermost object in `query`, offsets within `query`.
pub fn top_level_keys(query: &str) -> Vec<(usize, usize, String)> {
    let bytes = query.as_bytes();
    let mut keys = Vec::new();
    let (mut depth, mut i) = (0usize, 0usize);
    let mut expect_key = false;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                expect_key = depth == 1;
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            b'[' => depth += 1,
            b',' => expect_key = depth == 1,
            b'\'' | b'"' => {
                let quote = bytes[i];
                let start = i + 1;
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += usize::from(bytes[i] == b'\\') + 1;
                }
                if expect_key && depth == 1 && query[i.min(bytes.len())..].trim_start_matches(quote as char).trim_start().starts_with(':') {
                    keys.push((start, i, query[start..i].to_owned()));
                }
                expect_key = false;
            }
            c if expect_key && depth == 1 && (c.is_ascii_alphabetic() || c == b'_' || c == b'$') => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$' | b'.')) {
                    i += 1;
                }
                if query[i..].trim_start().starts_with(':') {
                    keys.push((start, i, query[start..i].to_owned()));
                }
                expect_key = false;
                continue;
            }
            c if !c.is_ascii_whitespace() => expect_key = false,
            _ => {}
        }
        i += 1;
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(q: &str) -> Vec<String> {
        top_level_keys(q).into_iter().map(|k| k.2).collect()
    }

    #[test]
    fn reads_quoted_and_bare_keys_of_the_outer_object() {
        assert_eq!(names("{ 'lastname' : ?0, \"age\": { $gt: ?1 }, address.city: ?2 }"), ["lastname", "age", "address.city"]);
        assert_eq!(names("{ $or: [ { a: 1 }, { b: 2 } ], name: ?0 }"), ["$or", "name"]);
        assert!(names("{}").is_empty());
        assert!(names("not json").is_empty());
    }

    #[test]
    fn key_offsets_cover_the_name() {
        let q = "{ 'last' : ?0 }";
        let (from, to, _) = top_level_keys(q)[0].clone();
        assert_eq!(&q[from..to], "last");
    }
}
