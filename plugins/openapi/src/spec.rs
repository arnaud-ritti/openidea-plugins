//! The OpenAPI document model: version detection and the operations under `paths`.

pub use spring_text::config::Key;
use std::collections::HashMap;

pub const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

pub struct Spec {
    pub version: String,
    pub keys: Vec<Key>,
}

pub struct Operation {
    pub method: String,
    pub path: String,
    pub key: String,
    pub at: usize,
    pub end: usize,
    pub operation_id: Option<String>,
}

pub fn parse(text: &str) -> Option<Spec> {
    let keys = spring_text::config::yaml(text);
    let version = keys
        .iter()
        .find(|k| k.key == "openapi" || k.key == "swagger")
        .map(|k| k.value.clone())
        .filter(|v| !v.is_empty())?;

    Some(Spec { version, keys })
}

/// The first key (in document order) for each exact key path.
pub fn exact(keys: &[Key]) -> HashMap<&str, &Key> {
    let mut by_key = HashMap::with_capacity(keys.len());
    for k in keys {
        by_key.entry(k.key.as_str()).or_insert(k);
    }

    by_key
}

pub fn operations(text: &str, keys: &[Key]) -> Vec<Operation> {
    let by_key = exact(keys);

    keys.iter()
        .filter_map(|k| {
            let method = &text[k.at..k.end];
            if !METHODS.contains(&method) {
                return None;
            }

            let path_key = k.key.strip_suffix(method)?.strip_suffix('.')?;
            let path_item = by_key.get(path_key)?;
            let path = &text[path_item.at..path_item.end];
            if !path.starts_with('/') || path_key != format!("paths.{path}") {
                return None;
            }

            Some(Operation {
                method: method.to_uppercase(),
                path: path.to_string(),
                key: k.key.clone(),
                at: k.at,
                end: k.end,
                operation_id: by_key.get(format!("{}.operationId", k.key).as_str()).map(|v| v.value.clone()),
            })
        })
        .collect()
}

pub struct Ref {
    pub target: String,
    pub at: usize,
    pub end: usize,
}

pub fn refs(text: &str) -> Vec<Ref> {
    let bytes = text.as_bytes();
    let skip_spaces = |mut i: usize| {
        while bytes.get(i) == Some(&b' ') {
            i += 1;
        }
        i
    };
    let line_end = |from: usize| text[from..].find(['\n', '\r']).map_or(text.len(), |n| from + n);

    let mut found = Vec::new();
    let mut from = 0;
    while let Some(n) = text[from..].find("$ref") {
        let found_at = from + n;
        let mut i = found_at + 4;
        from = i;
        let line_start = text[..found_at].rfind(['\n', '\r']).map_or(0, |n| n + 1);
        let before = &text[line_start..found_at];
        if before.match_indices('#').any(|(j, _)| j == 0 || before.as_bytes()[j - 1].is_ascii_whitespace()) {
            continue;
        }
        if matches!(bytes.get(i), Some(b'"' | b'\'')) {
            i += 1;
        }
        i = skip_spaces(i);
        if bytes.get(i) != Some(&b':') {
            continue;
        }
        let start = skip_spaces(i + 1);
        let eol = line_end(start);
        let (at, end) = match bytes.get(start) {
            Some(&quote @ (b'"' | b'\'')) => {
                let at = start + 1;
                (at, text[at..eol].find(quote as char).map_or(eol, |n| at + n))
            }
            _ => {
                let at = start;
                let stop = text[at..eol].find([',', '}']).map_or(eol, |n| at + n);
                let stop = text[at..stop].find(" #").map_or(stop, |n| at + n);
                (at, at + text[at..stop].trim_end_matches(' ').len())
            }
        };
        found.push(Ref { target: text[at..end].to_string(), at, end });
        from = end.max(from);
    }

    found
}

pub fn split_ref(target: &str) -> Option<(&str, &str)> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return None;
    }

    Some(target.split_once('#').unwrap_or((target, "")))
}

fn unescape_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = segment.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }

    String::from_utf8_lossy(&decoded).replace("~1", "/").replace("~0", "~")
}

fn escape_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Every key path prefix that ends at a `.` or `[` boundary, mapped to the first key under it.
pub struct Index<'k>(HashMap<&'k str, &'k Key>);

impl<'k> Index<'k> {
    pub fn new(keys: &'k [Key]) -> Self {
        let mut prefixes = HashMap::new();
        for k in keys {
            let boundaries = k.key.match_indices(['.', '[']).map(|(i, _)| i).chain([k.key.len()]);
            for end in boundaries {
                prefixes.entry(&k.key[..end]).or_insert(k);
            }
        }

        Self(prefixes)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.0.contains_key(path)
    }

    pub fn target(&self, fragment: &str) -> Option<&'k Key> {
        let mut path = String::new();
        for raw in fragment.strip_prefix('/')?.split('/') {
            let segment = unescape_segment(raw);
            let mut next = if path.is_empty() { segment.clone() } else { format!("{path}.{segment}") };
            if !self.contains(&next)
                && !path.is_empty()
                && !segment.is_empty()
                && segment.bytes().all(|b| b.is_ascii_digit())
            {
                next = format!("{path}[{segment}]");
            }
            if !self.contains(&next) {
                return None;
            }
            path = next;
        }

        self.0.get(path.as_str()).copied()
    }
}

pub fn target<'k>(keys: &'k [Key], fragment: &str) -> Option<&'k Key> {
    Index::new(keys).target(fragment)
}

pub fn ref_targets(text: &str, keys: &[Key]) -> Vec<String> {
    let is_fragment = !keys.iter().any(|k| k.key == "openapi" || k.key == "swagger");

    keys.iter()
        .filter_map(|k| {
            let segment = &text[k.at..k.end];
            let escaped = escape_segment(segment);
            let parent = k.key.strip_suffix(segment).and_then(|p| p.strip_suffix('.'));
            let Some(parent) = parent else {
                let top_level = !k.key.contains(['.', '[']);
                return (is_fragment && top_level).then(|| format!("#/{escaped}"));
            };

            let component = parent.strip_prefix("components.").is_some_and(|name| !name.contains('.'))
                || ["definitions", "parameters", "responses", "securityDefinitions"].contains(&parent);

            component.then(|| format!("#/{}/{escaped}", parent.replace('.', "/")))
        })
        .collect()
}

pub fn schema_url(version: &str) -> Option<&'static str> {
    match version {
        "2.0" => Some("https://spec.openapis.org/oas/2.0/schema/2017-08-27"),
        v if v.starts_with("3.0.") => Some("https://spec.openapis.org/oas/3.0/schema/2024-10-18"),
        v if v.starts_with("3.1.") => Some("https://spec.openapis.org/oas/3.1/schema/2025-11-23"),
        v if v.starts_with("3.2.") => Some("https://spec.openapis.org/oas/3.2/schema/2025-11-23"),
        _ => None,
    }
}

const MODELINE: &str = "# yaml-language-server: $schema=";

pub fn modeline(text: &str, version: &str) -> Option<String> {
    if text.contains(MODELINE) {
        return None;
    }

    schema_url(version).map(|url| format!("{MODELINE}{url}\n"))
}

#[cfg(test)]
pub(crate) const PETSTORE: &str = "openapi: 3.0.3
info:
  title: Pets
  version: 1.0.0
servers:
  - url: https://api.example.com/v1
paths:
  /pets:
    get:
      operationId: listPets
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: array
                items: { $ref: '#/components/schemas/Pet' }
  /pets/{petId}:
    parameters:
      - $ref: '#/components/parameters/PetId'
    get:
      operationId: showPet
      responses:
        '200':
          $ref: '#/components/responses/Pet'
    delete:
      operationId: deletePet
      parameters:
        - name: force
          in: query
      responses:
        '204':
          description: gone
components:
  schemas:
    Pet:
      type: object
      properties:
        id: { type: integer }
    com.acme.Error:
      type: object
  parameters:
    PetId:
      name: petId
      in: path
      required: true
      schema: { type: string }
  responses:
    Pet:
      description: a pet
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/Pet'
";

#[test]
fn detects_openapi_and_swagger_documents() {
    assert_eq!(parse(PETSTORE).unwrap().version, "3.0.3");
    assert_eq!(parse("swagger: \"2.0\"\ninfo: {}\n").unwrap().version, "2.0");
    assert!(parse("apiVersion: v1\nkind: Pod\n").is_none());
    assert!(parse("info:\n  openapi: 3.0.0\n").is_none());
    assert!(parse("openapi:\n").is_none());
}

#[test]
fn lists_operations_under_paths() {
    let spec = parse(PETSTORE).unwrap();
    let ops = operations(PETSTORE, &spec.keys);
    let summary: Vec<_> = ops.iter().map(|o| (o.method.as_str(), o.path.as_str(), o.operation_id.as_deref())).collect();
    assert_eq!(summary, [
        ("GET", "/pets", Some("listPets")),
        ("GET", "/pets/{petId}", Some("showPet")),
        ("DELETE", "/pets/{petId}", Some("deletePet")),
    ]);
    assert_eq!(&PETSTORE[ops[1].at..ops[1].end], "get");
    assert_eq!(ops[1].key, "paths./pets/{petId}.get");
}

#[test]
fn a_path_with_dots_keeps_its_method() {
    let text = "openapi: 3.1.0\npaths:\n  /v1.2/pets.json:\n    parameters: []\n    post:\n      responses: {}\n";
    let ops = operations(text, &parse(text).unwrap().keys);
    assert_eq!(ops.len(), 1);
    assert_eq!((ops[0].method.as_str(), ops[0].path.as_str(), ops[0].operation_id.as_deref()), ("POST", "/v1.2/pets.json", None));
}

#[test]
fn finds_refs_in_block_and_flow_style() {
    let found: Vec<_> = refs(PETSTORE).into_iter().map(|r| r.target).collect();
    assert_eq!(found, [
        "#/components/schemas/Pet",
        "#/components/parameters/PetId",
        "#/components/responses/Pet",
        "#/components/schemas/Pet",
    ]);
    let first = &refs(PETSTORE)[0];
    assert_eq!(&PETSTORE[first.at..first.end], "#/components/schemas/Pet");
    let text = "a: {\"$ref\": \"other.yaml#/X\", b: 1}\n";
    assert_eq!(refs(text)[0].target, "other.yaml#/X");
}

#[test]
fn an_unfinished_ref_ends_at_the_line_end() {
    let text = "a:\n  $ref: '#/comp\nb: 1\n";
    let r = &refs(text)[0];
    assert_eq!(&text[r.at..r.end], "#/comp");
    let r = &refs("$ref: ")[0];
    assert_eq!((r.at, r.end), (6, 6));
}

#[test]
fn splits_file_and_fragment() {
    assert_eq!(split_ref("#/components/schemas/Pet"), Some(("", "/components/schemas/Pet")));
    assert_eq!(split_ref("common.yaml#/Pet"), Some(("common.yaml", "/Pet")));
    assert_eq!(split_ref("../common.yaml"), Some(("../common.yaml", "")));
    assert_eq!(split_ref("https://example.com/a.yaml#/x"), None);
}

#[test]
fn resolves_json_pointers() {
    let keys = &parse(PETSTORE).unwrap().keys;
    let key = |p: &str| target(keys, p).map(|k| k.key.as_str());
    assert_eq!(key("/components/schemas/Pet"), Some("components.schemas.Pet"));
    assert_eq!(key("/components/schemas/com.acme.Error"), Some("components.schemas.com.acme.Error"));
    assert_eq!(key("/paths/~1pets~1%7BpetId%7D/get"), Some("paths./pets/{petId}.get"));
    assert_eq!(key("/paths/~1pets~1{petId}/parameters/0"), Some("paths./pets/{petId}.parameters[0].$ref"));
    assert_eq!(key("/paths/~1pets/get/responses/200"), Some("paths./pets.get.responses.200"));
    assert_eq!(key("/components/schemas/Pett"), None);
}

#[test]
fn offers_reusable_components_as_ref_targets() {
    let keys = &parse(PETSTORE).unwrap().keys;
    assert_eq!(ref_targets(PETSTORE, keys), [
        "#/components/schemas/Pet",
        "#/components/schemas/com.acme.Error",
        "#/components/parameters/PetId",
        "#/components/responses/Pet",
    ]);
    let swagger = "swagger: '2.0'\ndefinitions:\n  Pet:\n    type: object\n  a/b:\n    type: string\n";
    assert_eq!(ref_targets(swagger, &parse(swagger).unwrap().keys), ["#/definitions/Pet", "#/definitions/a~1b"]);
    let fragment = "Pet:\n  type: object\nError:\n  type: object\n";
    assert_eq!(ref_targets(fragment, &spring_text::config::yaml(fragment)), ["#/Pet", "#/Error"]);
}

#[test]
fn scanning_every_prefix_of_a_spec_never_panics() {
    let text = format!("{PETSTORE}x: {{ \"$ref\": 'é#/ü' }}\n$ref: \"日本");
    for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
        let prefix = &text[..end];
        for r in refs(prefix) {
            let _ = &prefix[r.at..r.end];
        }
        if let Some(spec) = parse(prefix) {
            ref_targets(prefix, &spec.keys);
            target(&spec.keys, "/paths/~1pets/get");
        }
    }
}

#[test]
fn commented_out_refs_are_not_refs() {
    let text = "# $ref: '#/a'\nx:\n  # $ref: '#/b'\n  $ref: '#/c' # $ref: '#/d'\ny: { $ref: '#/e#f' }\n";
    let found: Vec<_> = refs(text).into_iter().map(|r| r.target).collect();
    assert_eq!(found, ["#/c", "#/e#f"]);
}
