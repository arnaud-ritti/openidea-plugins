//! Edits that move schemas between places: extracting an inline schema into
//! `components/schemas`, bundling external `$ref`s into one document, and the
//! JSON pointer of the key under the caret. YAML block style only, except the pointer.

use crate::checks::Fetched;
use crate::spec::{self, Key};
use openidea_plugin::TextEdit;
use std::collections::{HashMap, HashSet};

const MAX_IMPORTS: usize = 200;

struct Lines<'t> {
    text: &'t str,
    starts: Vec<usize>,
}

impl<'t> Lines<'t> {
    fn new(text: &'t str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1).filter(|i| *i < text.len()));
        if text.is_empty() {
            starts.clear();
        }

        Self { text, starts }
    }

    fn len(&self) -> usize {
        self.starts.len()
    }

    fn get(&self, i: usize) -> &'t str {
        let end = self.starts.get(i + 1).copied().unwrap_or(self.text.len());

        self.text[self.starts[i]..end].trim_end_matches(['\n', '\r'])
    }

    fn after(&self, i: usize) -> usize {
        self.starts.get(i + 1).copied().unwrap_or(self.text.len())
    }

    fn indent(&self, i: usize) -> usize {
        let line = self.get(i);

        line.len() - line.trim_start_matches(' ').len()
    }

    fn skippable(&self, i: usize) -> bool {
        let line = self.get(i).trim();

        line.is_empty() || line.starts_with('#')
    }

    fn block_end(&self, i: usize) -> usize {
        let indent = self.indent(i);
        let mut last = i;
        for j in i + 1..self.len() {
            if self.skippable(j) {
                continue;
            }
            let list_of_parent = self.indent(j) == indent && self.get(j).trim_start().starts_with("- ");
            if self.indent(j) <= indent && !list_of_parent {
                break;
            }
            last = j;
        }

        last + 1
    }

    fn first_child_indent(&self, from: usize, to: usize) -> Option<usize> {
        (from..to).find(|i| !self.skippable(*i)).map(|i| self.indent(i))
    }
}

fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn entry(name: &str, body: &[String], pad: usize, eol: &str) -> String {
    let mut out = format!("{}{name}:{eol}", " ".repeat(pad));
    for line in body {
        match line.is_empty() {
            true => out.push_str(eol),
            false => out.push_str(&format!("{}{line}{eol}", " ".repeat(pad + 2))),
        }
    }

    out
}

fn is_key_line(line: &str, key: &str) -> Option<bool> {
    let rest = line.trim_start().strip_prefix(key)?.strip_prefix(':')?;

    Some(rest.trim().is_empty() || rest.trim_start().starts_with('#'))
}

/// Where to put `name: body` under `components.<section>`: the offset and the text to insert there.
fn component_insertion(text: &str, section: &str, name: &str, body: &[String]) -> Option<(usize, String)> {
    let lines = Lines::new(text);
    let eol = eol_of(text);
    let components = (0..lines.len()).find(|i| lines.indent(*i) == 0 && lines.get(*i).starts_with("components:"));
    let Some(c) = components else {
        let prefix = if text.is_empty() || text.ends_with('\n') { "" } else { eol };
        let insert = format!("{prefix}components:{eol}  {section}:{eol}{}", entry(name, body, 4, eol));

        return Some((text.len(), insert));
    };
    if is_key_line(lines.get(c), "components") != Some(true) {
        return None;
    }

    let end = lines.block_end(c);
    let child = lines.first_child_indent(c + 1, end).unwrap_or(2);
    let section_line = (c + 1..end).find(|i| lines.indent(*i) == child && lines.get(*i).trim_start().starts_with(&format!("{section}:")));
    let (last, content) = match section_line {
        Some(s) => {
            if is_key_line(lines.get(s), section) != Some(true) {
                return None;
            }
            let send = lines.block_end(s);
            let pad = lines.first_child_indent(s + 1, send).unwrap_or(child + 2);

            (send - 1, entry(name, body, pad, eol))
        }
        None => (end - 1, format!("{}{section}:{eol}{}", " ".repeat(child), entry(name, body, child + 2, eol))),
    };
    let offset = lines.after(last);
    let prefix = if offset == text.len() && !text.ends_with('\n') { eol } else { "" };

    Some((offset, format!("{prefix}{content}")))
}

fn pascal(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect()
}

fn last_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn parent_name(path: &str, own: &str) -> String {
    let parent = path.strip_suffix(own).and_then(|p| p.strip_suffix('.')).unwrap_or(path);

    pascal(last_segment(parent))
}

fn schema_names(keys: &[Key], text: &str) -> HashSet<String> {
    let json = spec::is_json(text);
    keys.iter()
        .filter_map(|k| {
            let rest = k.key.strip_prefix("components.schemas.")?;
            let own = &text[k.at..k.end];
            let own = if json { crate::json::unescape(own) } else { own.to_owned() };

            (rest == own).then_some(own)
        })
        .collect()
}

fn unique(base: &str, taken: &HashSet<String>) -> String {
    let base = if base.is_empty() { "Schema" } else { base };
    if !taken.contains(base) {
        return base.to_owned();
    }

    (2..).map(|n| format!("{base}{n}")).find(|candidate| !taken.contains(candidate)).unwrap_or_default()
}

fn suggested_name(text: &str, keys: &[Key], key: &Key, own: &str) -> String {
    match own {
        "schema" => {
            let operation = spec::operations(text, keys).into_iter().find(|op| key.key.starts_with(&format!("{}.", op.key)));
            let base = operation.map(|op| pascal(&op.operation_id.unwrap_or_else(|| format!("{} {}", op.method, op.path)))).unwrap_or_default();
            let suffix = if key.key.contains(".requestBody.") {
                "Request"
            } else if key.key.contains(".responses.") {
                "Response"
            } else if key.key.contains(".parameters[") {
                "Parameter"
            } else {
                "Schema"
            };

            format!("{base}{suffix}")
        }
        "items" => format!("{}Item", parent_name(&key.key, own)),
        "additionalProperties" => format!("{}Value", parent_name(&key.key, own)),
        _ => pascal(own),
    }
}

fn is_inline_schema(text: &str, keys: &[Key], key: &Key) -> bool {
    let own = &text[key.at..key.end];
    let parent = key.key.strip_suffix(own).and_then(|p| p.strip_suffix('.')).unwrap_or("");

    matches!(own, "schema" | "items" | "additionalProperties") && !parent.is_empty()
        || last_segment(parent) == "properties" && keys.iter().any(|k| k.key == parent)
}

/// The edits that move the inline schema whose key is on `line` into `components.schemas`.
pub fn extract_schema(text: &str, keys: &[Key], line: u32) -> Option<Vec<TextEdit>> {
    if spec::is_json(text) {
        return None;
    }

    let lines = Lines::new(text);
    let i = line as usize;
    if i >= lines.len() || lines.get(i).trim_start().starts_with("- ") {
        return None;
    }

    let start = lines.starts[i];
    let key = keys.iter().find(|k| k.at >= start && k.at < lines.after(i) && k.value.is_empty())?;
    let own = &text[key.at..key.end];
    if !is_inline_schema(text, keys, key) || is_key_line(lines.get(i), own) != Some(true) {
        return None;
    }

    let end = lines.block_end(i);
    let first = (i + 1..end).find(|j| !lines.skippable(*j))?;
    let content = lines.get(first).trim_start();
    if content.starts_with("- ") || (content.starts_with("$ref:") && (first + 1..end).all(|j| lines.skippable(j))) {
        return None;
    }

    let indent = (i + 1..end).filter(|j| !lines.skippable(*j)).map(|j| lines.indent(j)).min()?;
    let body: Vec<String> = (i + 1..end)
        .map(|j| lines.get(j).get(indent..).unwrap_or("").to_owned())
        .collect();
    let name = unique(&suggested_name(text, keys, key, own), &schema_names(keys, text));
    let (insert_at, insert) = component_insertion(text, "schemas", &name, &body)?;

    let eol = eol_of(text);
    let from = lines.starts[i + 1];
    let to = lines.after(end - 1);
    if insert_at > from && insert_at < to {
        return None;
    }

    let terminated = text[from..to].ends_with('\n');
    let mut replacement = format!("{}$ref: '#/components/schemas/{name}'", " ".repeat(indent));
    if terminated {
        replacement.push_str(eol);
    }
    if insert_at == to {
        replacement.push_str(&insert);
        return Some(vec![TextEdit { range: spring_text::range(text, from, to), text: replacement }]);
    }

    let mut edits = vec![
        TextEdit { range: spring_text::range(text, from, to), text: replacement },
        TextEdit { range: spring_text::range(text, insert_at, insert_at), text: insert },
    ];
    edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));

    Some(edits)
}

struct Bundle<'r> {
    read: &'r dyn Fn(&str) -> Fetched,
    assigned: HashMap<(String, String), (String, String)>,
    taken: HashMap<String, HashSet<String>>,
    entries: Vec<(String, String, Vec<String>)>,
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect()
}

fn unescape_pointer(segment: &str) -> String {
    segment.replace("~1", "/").replace("~0", "~")
}

impl Bundle<'_> {
    fn import(&mut self, file: &str, pointer: &str) -> Result<(String, String), String> {
        let id = (file.to_owned(), pointer.to_owned());
        if let Some(found) = self.assigned.get(&id) {
            return Ok(found.clone());
        }
        if self.assigned.len() >= MAX_IMPORTS {
            return Err(format!("More than {MAX_IMPORTS} referenced objects."));
        }

        let content = match (self.read)(file) {
            Fetched::Text(content) => content,
            Fetched::Missing => return Err(format!("Cannot find file `{file}`.")),
            Fetched::Unreadable => return Err(format!("Cannot read `{file}`.")),
        };
        if spec::is_json(&content) {
            return Err(format!("`{file}` is JSON; only YAML files can be bundled."));
        }

        let body = pointed_body(&content, pointer).ok_or_else(|| format!("Cannot find `{pointer}` in `{file}` as a block mapping."))?;
        let rest = pointer.strip_prefix("/components/").unwrap_or("");
        let section = match rest.split_once('/') {
            Some((section, name)) if !name.is_empty() && !name.contains('/') => section.to_owned(),
            _ => "schemas".to_owned(),
        };
        let raw_name = pointer.rsplit('/').next().filter(|s| !s.is_empty()).map(unescape_pointer).unwrap_or_else(|| {
            let name = file.rsplit('/').next().unwrap_or(file);
            name.split('.').next().unwrap_or(name).to_owned()
        });
        let taken = self.taken.entry(section.clone()).or_default();
        let name = unique(&sanitize(&raw_name), taken);
        taken.insert(name.clone());
        self.assigned.insert(id, (section.clone(), name.clone()));

        let joined = body.join("\n");
        let mut rewritten = joined.clone();
        let mut found = spec::refs(&joined);
        found.reverse();
        for r in found {
            let Some((target_file, fragment)) = spec::split_ref(&r.target) else { continue };
            let next_file = if target_file.is_empty() { file.to_owned() } else { crate::resolve_path(file, target_file) };
            let (next_section, next_name) = self.import(&next_file, fragment)?;
            rewritten.replace_range(r.at..r.end, &format!("#/components/{next_section}/{next_name}"));
        }

        let slot = self.entries.len();
        self.entries.push((section.clone(), name.clone(), Vec::new()));
        self.entries[slot].2 = rewritten.split('\n').map(str::to_owned).collect();

        Ok((section, name))
    }
}

fn pointed_body(content: &str, pointer: &str) -> Option<Vec<String>> {
    let lines = Lines::new(content);
    let (from, to) = if pointer.is_empty() {
        (0, lines.len())
    } else {
        let keys = spec::scan(content);
        let key = spec::target(&keys, pointer).filter(|k| k.value.is_empty())?;
        let i = (0..lines.len()).find(|i| lines.starts[*i] <= key.at && key.at < lines.after(*i))?;

        (i + 1, lines.block_end(i))
    };
    let indent = (from..to).filter(|j| !lines.skippable(*j)).map(|j| lines.indent(j)).min()?;
    if (from..to).find(|j| !lines.skippable(*j)).is_some_and(|j| lines.get(j).trim_start().starts_with("- ")) {
        return None;
    }

    Some((from..to).map(|j| lines.get(j).get(indent..).unwrap_or("").to_owned()).collect())
}

/// `text` with every `$ref` to another file replaced by a pointer into `components`, and the
/// referenced objects copied there. Recursive references are fine: an object is copied once.
pub fn bundle(text: &str, path: &str, read: &dyn Fn(&str) -> Fetched) -> Result<Option<String>, String> {
    if spec::is_json(text) {
        return Err("Only YAML files can be bundled.".into());
    }

    let keys = spec::scan(text);
    let mut bundle = Bundle { read, assigned: HashMap::new(), taken: HashMap::new(), entries: Vec::new() };
    for k in &keys {
        let Some(rest) = k.key.strip_prefix("components.") else { continue };
        if let Some((section, name)) = rest.split_once('.') {
            if !name.contains(['.', '[']) {
                bundle.taken.entry(section.to_owned()).or_default().insert(text[k.at..k.end].to_owned());
            }
        }
    }

    let mut result = text.to_owned();
    let mut found = spec::refs(text);
    found.reverse();
    let mut changed = false;
    for r in found {
        let Some((file, fragment)) = spec::split_ref(&r.target) else { continue };
        if file.is_empty() {
            continue;
        }

        let (section, name) = bundle.import(&crate::resolve_path(path, file), fragment)?;
        result.replace_range(r.at..r.end, &format!("#/components/{section}/{name}"));
        changed = true;
    }
    if !changed {
        return Ok(None);
    }

    for (section, name, body) in &bundle.entries {
        let (at, insert) = component_insertion(&result, section, name, body).ok_or("Cannot add to this file's `components`.")?;
        result.insert_str(at, &insert);
    }

    Ok(Some(result))
}

fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// The JSON pointer (`#/components/schemas/Pet`) of the first key on `line`.
pub fn pointer_at(text: &str, keys: &[Key], line: u32) -> Option<String> {
    let lines = Lines::new(text);
    let i = line as usize;
    if i >= lines.len() {
        return None;
    }

    let start = lines.starts[i];
    let key = keys.iter().find(|k| k.at >= start && k.at < lines.after(i))?;
    let json = spec::is_json(text);
    let by_key = spec::exact(keys);
    let path = key.key.as_str();
    let mut segments = Vec::new();
    let mut at = 0;
    while at < path.len() {
        match path.as_bytes()[at] {
            b'.' => at += 1,
            b'[' => {
                let close = path[at..].find(']')? + at;
                segments.push(path[at + 1..close].to_owned());
                at = close + 1;
            }
            _ => {
                let end = (at + 1..=path.len())
                    .filter(|e| *e == path.len() || matches!(path.as_bytes()[*e], b'.' | b'['))
                    .find(|e| by_key.contains_key(&path[..*e]))?;
                let own = by_key[&path[..end]];
                let raw = &text[own.at..own.end];
                let raw = if json { crate::json::unescape(raw) } else { raw.to_owned() };
                segments.push(escape_pointer(&raw));
                at = end;
            }
        }
    }

    Some(format!("#/{}", segments.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::text::offset;

    fn apply(text: &str, edits: &[TextEdit]) -> String {
        let mut out = text.to_owned();
        let mut sorted: Vec<&TextEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse((e.range.start.line, e.range.start.character)));
        for e in sorted {
            out.replace_range(offset(text, e.range.start).unwrap()..offset(text, e.range.end).unwrap(), &e.text);
        }

        out
    }

    fn extract(text: &str, needle: &str) -> Option<String> {
        let line = text[..text.find(needle).unwrap()].matches('\n').count() as u32;
        let keys = spec::scan(text);

        extract_schema(text, &keys, line).map(|edits| apply(text, &edits))
    }

    const SPEC: &str = "openapi: 3.0.0
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
                type: object
                properties:
                  id:
                    type: integer
components:
  schemas:
    Pet:
      type: object
  parameters:
    Limit:
      name: limit
";

    #[test]
    fn extracts_an_inline_schema_into_the_existing_schemas() {
        let out = extract(SPEC, "schema:\n                type").unwrap();
        assert!(out.contains("              schema:\n                $ref: '#/components/schemas/ListPetsResponse'\n"), "{out}");
        assert!(out.contains("    Pet:\n      type: object\n    ListPetsResponse:\n      type: object\n      properties:\n        id:\n          type: integer\n  parameters:"), "{out}");
        let keys = spec::scan(&out);
        assert!(spec::target(&keys, "/components/schemas/ListPetsResponse/properties/id").is_some());
        assert!(spec::parse(&out).is_some());
    }

    #[test]
    fn creates_schemas_or_components_when_missing() {
        let no_schemas = SPEC.replace("  schemas:\n    Pet:\n      type: object\n", "");
        let out = extract(&no_schemas, "schema:\n                type").unwrap();
        assert!(out.ends_with("      name: limit\n  schemas:\n    ListPetsResponse:\n      type: object\n      properties:\n        id:\n          type: integer\n"), "{out}");

        let bare = SPEC[..SPEC.find("components:").unwrap()].to_owned();
        let out = extract(&bare, "schema:\n                type").unwrap();
        assert!(out.ends_with("components:\n  schemas:\n    ListPetsResponse:\n      type: object\n      properties:\n        id:\n          type: integer\n"), "{out}");
        assert!(out.contains("$ref: '#/components/schemas/ListPetsResponse'\ncomponents:") || out.contains("$ref: '#/components/schemas/ListPetsResponse'\n"));
    }

    #[test]
    fn extracts_a_property_and_avoids_taken_names() {
        let text = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Owner:\n      type: object\n    Pet:\n      type: object\n      properties:\n        owner:\n          type: object\n          properties:\n            name:\n              type: string\n";
        let out = extract(text, "owner:\n          type").unwrap();
        assert!(out.contains("        owner:\n          $ref: '#/components/schemas/Owner2'\n"), "{out}");
        assert!(out.ends_with("    Owner2:\n      type: object\n      properties:\n        name:\n          type: string\n"), "{out}");
    }

    #[test]
    fn refuses_refs_scalars_json_and_other_keys() {
        let ref_only = "openapi: 3.0.0\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          content:\n            a/b:\n              schema:\n                $ref: '#/components/schemas/Pet'\n";
        assert!(extract(ref_only, "schema:").is_none());
        assert!(extract(SPEC, "operationId").is_none());
        assert!(extract(SPEC, "content:").is_none());
        assert!(extract(SPEC, "Pet:").is_none());
        assert!(extract_schema("{\"openapi\": \"3.0.0\"}", &[], 0).is_none());
    }

    fn files(entries: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Fetched {
        move |file| match entries.iter().find(|(name, _)| *name == file) {
            Some((_, content)) => Fetched::Text((*content).to_owned()),
            None => Fetched::Missing,
        }
    }

    #[test]
    fn bundles_external_refs_into_components() {
        let main = "openapi: 3.0.0\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          content:\n            a/b:\n              schema:\n                $ref: './common.yaml#/Pet'\ncomponents:\n  schemas:\n    Own:\n      type: object\n";
        let read = files(&[("/p/common.yaml", "Pet:\n  type: object\n  properties:\n    tag:\n      $ref: '#/Tag'\n    friend:\n      $ref: '#/Pet'\nTag:\n  type: string\n")]);
        let out = bundle(main, "/p/api.yaml", &read).unwrap().unwrap();
        assert!(out.contains("$ref: '#/components/schemas/Pet'"), "{out}");
        assert!(out.contains("    Pet:\n      type: object\n      properties:\n        tag:\n          $ref: '#/components/schemas/Tag'\n        friend:\n          $ref: '#/components/schemas/Pet'\n"), "{out}");
        assert!(out.contains("    Tag:\n      type: string\n"), "{out}");
        assert!(spec::refs(&out).iter().all(|r| r.target.starts_with("#/components/schemas/")));
        let keys = spec::scan(&out);
        assert!(spec::refs(&out).iter().all(|r| spec::target(&keys, &r.target[1..]).is_some()));
    }

    #[test]
    fn bundling_reports_missing_files_json_and_nothing_to_do() {
        let main = "openapi: 3.0.0\nx:\n  $ref: './nope.yaml#/A'\n";
        assert!(bundle(main, "/p/api.yaml", &files(&[])).unwrap_err().contains("nope.yaml"));
        assert!(bundle("{\"openapi\": \"3.0.0\"}", "/p/api.json", &files(&[])).is_err());
        assert_eq!(bundle("openapi: 3.0.0\nx:\n  $ref: '#/A'\n", "/p/api.yaml", &files(&[])).unwrap(), None);
    }

    #[test]
    fn pointer_of_the_key_on_a_line() {
        let keys = spec::scan(SPEC);
        let line = |needle: &str| SPEC[..SPEC.find(needle).unwrap()].matches('\n').count() as u32;
        assert_eq!(pointer_at(SPEC, &keys, line("    Pet:")).unwrap(), "#/components/schemas/Pet");
        assert_eq!(pointer_at(SPEC, &keys, line("  /pets")).unwrap(), "#/paths/~1pets");
        assert_eq!(pointer_at(SPEC, &keys, line("        '200'")).unwrap(), "#/paths/~1pets/get/responses/200");
        let list = "openapi: 3.0.0\nx:\n  - name: a\n  - name: b\n";
        assert_eq!(pointer_at(list, &spec::scan(list), 3).unwrap(), "#/x/1/name");
        let json = "{\"openapi\": \"3.0.0\",\n \"a/b\": {\"c\": 1}}";
        assert_eq!(pointer_at(json, &spec::scan(json), 1).unwrap(), "#/a~1b");
    }
}
