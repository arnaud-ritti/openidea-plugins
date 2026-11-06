//! Keyword and enum completion for OpenAPI 3 documents in YAML block style, from the
//! fixed object tree of the OpenAPI Specification chosen by the indentation of the caret's line.

enum Kind {
    Obj(&'static str),
    Map(&'static str, &'static [&'static str]),
    Val(&'static [&'static str]),
}

use Kind::{Map, Obj, Val};

const BOOL: Kind = Val(&["true", "false"]);
const FREE: Kind = Val(&[]);
const STYLES: &[&str] = &["matrix", "label", "form", "simple", "spaceDelimited", "pipeDelimited", "deepObject"];
const MEDIA_TYPES: &[&str] = &["application/json", "application/xml", "application/x-www-form-urlencoded", "multipart/form-data", "text/plain"];
const STATUS_CODES: &[&str] = &["200", "201", "202", "204", "400", "401", "403", "404", "409", "422", "500", "default"];
const NO_NAMES: &[&str] = &[];

fn node(name: &str) -> &'static [(&'static str, Kind)] {
    match name {
        "Root" => &[
            ("openapi", FREE),
            ("info", Obj("Info")),
            ("jsonSchemaDialect", FREE),
            ("servers", Obj("Server")),
            ("paths", Map("PathItem", NO_NAMES)),
            ("webhooks", Map("PathItem", NO_NAMES)),
            ("components", Obj("Components")),
            ("security", Obj("None")),
            ("tags", Obj("Tag")),
            ("externalDocs", Obj("ExternalDocs")),
        ],
        "Info" => &[
            ("title", FREE),
            ("summary", FREE),
            ("description", FREE),
            ("termsOfService", FREE),
            ("contact", Obj("Contact")),
            ("license", Obj("License")),
            ("version", FREE),
        ],
        "Contact" => &[("name", FREE), ("url", FREE), ("email", FREE)],
        "License" => &[("name", FREE), ("identifier", FREE), ("url", FREE)],
        "Server" => &[("url", FREE), ("description", FREE), ("variables", Map("ServerVariable", NO_NAMES))],
        "ServerVariable" => &[("enum", FREE), ("default", FREE), ("description", FREE)],
        "ExternalDocs" => &[("description", FREE), ("url", FREE)],
        "Tag" => &[("name", FREE), ("description", FREE), ("externalDocs", Obj("ExternalDocs"))],
        "PathItem" => &[
            ("$ref", FREE),
            ("summary", FREE),
            ("description", FREE),
            ("get", Obj("Operation")),
            ("put", Obj("Operation")),
            ("post", Obj("Operation")),
            ("delete", Obj("Operation")),
            ("options", Obj("Operation")),
            ("head", Obj("Operation")),
            ("patch", Obj("Operation")),
            ("trace", Obj("Operation")),
            ("servers", Obj("Server")),
            ("parameters", Obj("Parameter")),
        ],
        "Operation" => &[
            ("tags", FREE),
            ("summary", FREE),
            ("description", FREE),
            ("externalDocs", Obj("ExternalDocs")),
            ("operationId", FREE),
            ("parameters", Obj("Parameter")),
            ("requestBody", Obj("RequestBody")),
            ("responses", Map("Response", STATUS_CODES)),
            ("callbacks", Obj("None")),
            ("deprecated", BOOL),
            ("security", Obj("None")),
            ("servers", Obj("Server")),
        ],
        "Parameter" => &[
            ("$ref", FREE),
            ("name", FREE),
            ("in", Val(&["query", "header", "path", "cookie"])),
            ("description", FREE),
            ("required", BOOL),
            ("deprecated", BOOL),
            ("allowEmptyValue", BOOL),
            ("style", Val(STYLES)),
            ("explode", BOOL),
            ("allowReserved", BOOL),
            ("schema", Obj("Schema")),
            ("example", FREE),
            ("examples", Map("Example", NO_NAMES)),
            ("content", Map("MediaType", MEDIA_TYPES)),
        ],
        "RequestBody" => &[
            ("$ref", FREE),
            ("description", FREE),
            ("content", Map("MediaType", MEDIA_TYPES)),
            ("required", BOOL),
        ],
        "MediaType" => &[
            ("schema", Obj("Schema")),
            ("example", FREE),
            ("examples", Map("Example", NO_NAMES)),
            ("encoding", Map("Encoding", NO_NAMES)),
        ],
        "Encoding" => &[
            ("contentType", FREE),
            ("headers", Map("Header", NO_NAMES)),
            ("style", Val(STYLES)),
            ("explode", BOOL),
            ("allowReserved", BOOL),
        ],
        "Response" => &[
            ("$ref", FREE),
            ("description", FREE),
            ("headers", Map("Header", NO_NAMES)),
            ("content", Map("MediaType", MEDIA_TYPES)),
            ("links", Obj("None")),
        ],
        "Header" => &[
            ("$ref", FREE),
            ("description", FREE),
            ("required", BOOL),
            ("deprecated", BOOL),
            ("style", Val(&["simple"])),
            ("explode", BOOL),
            ("schema", Obj("Schema")),
            ("example", FREE),
            ("examples", Map("Example", NO_NAMES)),
            ("content", Map("MediaType", MEDIA_TYPES)),
        ],
        "Example" => &[
            ("$ref", FREE),
            ("summary", FREE),
            ("description", FREE),
            ("value", FREE),
            ("externalValue", FREE),
        ],
        "Components" => &[
            ("schemas", Map("Schema", NO_NAMES)),
            ("responses", Map("Response", NO_NAMES)),
            ("parameters", Map("Parameter", NO_NAMES)),
            ("examples", Map("Example", NO_NAMES)),
            ("requestBodies", Map("RequestBody", NO_NAMES)),
            ("headers", Map("Header", NO_NAMES)),
            ("securitySchemes", Map("SecurityScheme", NO_NAMES)),
            ("pathItems", Map("PathItem", NO_NAMES)),
        ],
        "SecurityScheme" => &[
            ("$ref", FREE),
            ("type", Val(&["apiKey", "http", "oauth2", "openIdConnect", "mutualTLS"])),
            ("description", FREE),
            ("name", FREE),
            ("in", Val(&["query", "header", "cookie"])),
            ("scheme", Val(&["basic", "bearer", "digest"])),
            ("bearerFormat", FREE),
            ("flows", Obj("OAuthFlows")),
            ("openIdConnectUrl", FREE),
        ],
        "OAuthFlows" => &[
            ("implicit", Obj("OAuthFlow")),
            ("password", Obj("OAuthFlow")),
            ("clientCredentials", Obj("OAuthFlow")),
            ("authorizationCode", Obj("OAuthFlow")),
        ],
        "OAuthFlow" => &[
            ("authorizationUrl", FREE),
            ("tokenUrl", FREE),
            ("refreshUrl", FREE),
            ("scopes", Obj("None")),
        ],
        "Schema" => &[
            ("$ref", FREE),
            ("type", Val(&["array", "boolean", "integer", "null", "number", "object", "string"])),
            (
                "format",
                Val(&[
                    "int32", "int64", "float", "double", "byte", "binary", "date", "date-time", "password", "email", "uuid", "uri",
                    "hostname", "ipv4", "ipv6",
                ]),
            ),
            ("title", FREE),
            ("description", FREE),
            ("default", FREE),
            ("example", FREE),
            ("enum", FREE),
            ("const", FREE),
            ("nullable", BOOL),
            ("readOnly", BOOL),
            ("writeOnly", BOOL),
            ("deprecated", BOOL),
            ("required", FREE),
            ("properties", Map("Schema", NO_NAMES)),
            ("additionalProperties", Obj("Schema")),
            ("items", Obj("Schema")),
            ("allOf", Obj("Schema")),
            ("anyOf", Obj("Schema")),
            ("oneOf", Obj("Schema")),
            ("not", Obj("Schema")),
            ("minimum", FREE),
            ("maximum", FREE),
            ("exclusiveMinimum", FREE),
            ("exclusiveMaximum", FREE),
            ("multipleOf", FREE),
            ("minLength", FREE),
            ("maxLength", FREE),
            ("pattern", FREE),
            ("minItems", FREE),
            ("maxItems", FREE),
            ("uniqueItems", BOOL),
            ("minProperties", FREE),
            ("maxProperties", FREE),
            ("discriminator", Obj("Discriminator")),
            ("externalDocs", Obj("ExternalDocs")),
        ],
        "Discriminator" => &[("propertyName", FREE), ("mapping", Obj("None"))],
        _ => &[],
    }
}

pub struct Key {
    pub label: &'static str,
    pub insert: String,
}

pub enum Completion {
    Keys { items: Vec<Key>, from: usize },
    Values { items: Vec<&'static str>, from: usize },
}

struct Line<'t> {
    dash: Option<usize>,
    key_indent: usize,
    key: Option<&'t str>,
    value: &'t str,
}

fn parse_line(raw: &str) -> Option<Line<'_>> {
    let trimmed = raw.trim_start_matches(' ');
    if trimmed.is_empty() || trimmed.starts_with('#') || raw.contains('\t') {
        return None;
    }

    let indent = raw.len() - trimmed.len();
    let (dash, rest, key_indent) = match trimmed.strip_prefix("- ") {
        Some(rest) => {
            let after = rest.trim_start_matches(' ');
            (Some(indent), after, indent + 2 + rest.len() - after.len())
        }
        None => (None, trimmed, indent),
    };
    let colon = rest.match_indices(':').find(|(i, _)| rest[i + 1..].is_empty() || rest[i + 1..].starts_with(' '));
    let Some((colon, _)) = colon else {
        return Some(Line { dash, key_indent, key: None, value: rest });
    };

    Some(Line { dash, key_indent, key: Some(rest[..colon].trim_matches(['\'', '"'])), value: rest[colon + 1..].trim() })
}

enum Place {
    Node(&'static str),
    Names(&'static [&'static str]),
}

fn resolve(path: &[&str]) -> Option<Place> {
    let mut name = "Root";
    let mut at = 0;
    while at < path.len() {
        match &node(name).iter().find(|(key, _)| *key == path[at])?.1 {
            Obj(next) => {
                name = next;
                at += 1;
            }
            Map(_, names) if at + 1 == path.len() => return Some(Place::Names(names)),
            Map(next, _) => {
                name = next;
                at += 2;
            }
            Val(_) => return None,
        }
    }

    Some(Place::Node(name))
}

pub fn complete(text: &str, offset: usize, version: &str) -> Option<Completion> {
    if !version.starts_with('3') {
        return None;
    }

    let line_start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let line = text[..offset].matches('\n').count();
    let lines: Vec<&str> = text.lines().collect();
    let before = &text[line_start..offset];
    let trimmed = before.trim_start_matches(' ');
    if before.contains('\t') || trimmed.starts_with('#') {
        return None;
    }

    let indent = before.len() - trimmed.len();
    let (key_indent, rest) = match trimmed.strip_prefix("- ") {
        Some(rest) => {
            let after = rest.trim_start_matches(' ');
            (indent + 2 + rest.len() - after.len(), after)
        }
        None if trimmed == "-" => return None,
        None => (indent, trimmed),
    };

    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '$'));
    let is_key = rest.is_empty() || word(rest);
    let value_of = rest.split_once(": ");
    if !is_key && !value_of.is_some_and(|(key, partial)| word(key) && (partial.is_empty() || word(partial))) {
        return None;
    }

    let mut path: Vec<&str> = Vec::new();
    let mut limit = key_indent;
    for raw in lines.iter().take(line).rev() {
        if raw.trim_end() == "---" {
            break;
        }
        let Some(above) = parse_line(raw) else { continue };
        let Some(key) = above.key else { continue };
        match above.dash {
            Some(dash) if above.key_indent == limit => limit = dash + 1,
            Some(dash) if above.key_indent < limit => {
                if !above.value.is_empty() {
                    return None;
                }
                path.push(key);
                limit = dash + 1;
            }
            None if above.key_indent < limit => {
                if !above.value.is_empty() {
                    return None;
                }
                path.push(key);
                limit = above.key_indent;
            }
            _ => {}
        }
    }
    path.reverse();

    let place = resolve(&path)?;
    let from = offset - rest.len();
    if let Some((key, _)) = value_of.filter(|_| !is_key) {
        let Place::Node(name) = place else { return None };
        let Val(values) = &node(name).iter().find(|(k, _)| *k == key)?.1 else { return None };
        let from = from + key.len() + 2;
        return (!values.is_empty()).then(|| Completion::Values { items: values.to_vec(), from });
    }

    let items = match place {
        Place::Node(name) => node(name)
            .iter()
            .map(|(label, kind)| Key {
                label,
                insert: if matches!(kind, Val(_)) { format!("{label}: ") } else { format!("{label}:") },
            })
            .collect(),
        Place::Names(names) => names
            .iter()
            .map(|label| Key { label, insert: format!("'{label}':").replace("'default'", "default") })
            .collect(),
    };

    Some(Completion::Keys { items, from })
}
