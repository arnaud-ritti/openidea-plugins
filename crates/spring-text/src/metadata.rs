//! The config-key catalog (what keys exist, their types, defaults, deprecations and
//! value hints) and the `.properties` / YAML completion built on it. Shared by the
//! plugins of Spring Boot and Quarkus; MicroProfile `%profile.` prefixes are skipped.

use crate::config::{canonical, yaml_parent};
use crate::range;
use openidea_plugin::Range;
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::{BTreeMap, BTreeSet};

/// The type of a prefix bound to something we can't see (a `@Bean` method's
/// return type, a superclass): any key below it is accepted.
const OPEN: &str = "*";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prop {
    pub name: String,
    /// Simple type names: `Map<String,String>`.
    pub ty: String,
    pub default: String,
    pub deprecated: Option<Deprecation>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deprecation {
    /// Level `error`: the key no longer does anything.
    pub removed: bool,
    /// May be empty.
    pub replacement: String,
}

impl Prop {
    /// A map (or an open prefix): any key below it binds.
    pub fn open(&self) -> bool {
        self.ty.starts_with("Map<") || self.ty == "Properties" || self.ty == OPEN
    }

    pub fn boolean(&self) -> bool {
        matches!(self.ty.as_str(), "Boolean" | "boolean")
    }

    pub fn open_prefix(name: String) -> Self {
        Prop { name, ty: OPEN.into(), ..Prop::default() }
    }
}

/// What one source declares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub props: Vec<Prop>,
    /// Key (`logging.level.values` for a map's values) → its values.
    pub hints: Vec<(String, Vec<String>)>,
}

#[derive(Default)]
pub struct Catalog {
    /// Canonical name → property.
    props: BTreeMap<String, Prop>,
    hints: BTreeMap<String, Vec<String>>,
    /// Canonical names of the groups of two segments or more properties are in.
    groups: BTreeSet<String>,
}

impl Catalog {
    pub fn add(&mut self, meta: &Meta) {
        for p in &meta.props {
            let name = canonical(&p.name);
            let mut group = name.as_str();
            while let Some((parent, _)) = group.rsplit_once('.') {
                if parent.contains('.') {
                    self.groups.insert(parent.to_owned());
                }
                group = parent;
            }
            self.props.entry(name).or_insert_with(|| p.clone());
        }
        for (name, values) in &meta.hints {
            self.hints.entry(canonical(name)).or_insert_with(|| values.clone());
        }
    }
}

/// The TSV a plugin's `scripts/gen-metadata.py` writes.
pub fn parse_tsv(text: &str) -> Meta {
    let mut meta = Meta::default();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let mut f = line.split('\t');
        let name = f.next().unwrap_or_default();
        if let Some(hint) = name.strip_prefix('@') {
            meta.hints.push((hint.to_owned(), f.map(str::to_owned).collect()));
            continue;
        }
        let mut next = || f.next().unwrap_or_default().to_owned();
        let (ty, default, deprecation, description) = (next(), next(), next(), next());
        let deprecated = deprecation.split_once(':').map(|(level, replacement)| Deprecation {
            removed: level == "e",
            replacement: replacement.to_owned(),
        });
        meta.props.push(Prop { name: name.to_owned(), ty, default, deprecated, description });
    }
    meta
}

/// Several catalogs, the first ones winning.
pub struct Lookup<'a>(pub Vec<&'a Catalog>);

impl Lookup<'_> {
    pub fn prop(&self, key: &str) -> Option<&Prop> {
        let key = canonical(key);
        self.0.iter().find_map(|c| c.props.get(&key))
    }

    fn group(&self, canonical: &str) -> bool {
        self.0.iter().any(|c| c.groups.contains(canonical))
    }

    /// The map property `key` is an entry of (`logging.level` for `logging.level.com.acme`).
    pub fn map_of(&self, key: &str) -> Option<&Prop> {
        let key = canonical(key);
        let mut at = key.len();
        while let Some(dot) = key[..at].rfind('.') {
            if let Some(p) = self.0.iter().find_map(|c| c.props.get(&key[..dot])).filter(|p| p.open()) {
                return Some(p);
            }
            at = dot;
        }
        None
    }

    fn hints(&self, key: &str) -> Option<&Vec<String>> {
        let key = canonical(key);
        self.0.iter().find_map(|c| c.hints.get(&key))
    }

    /// The values `key` can take: its hints, the map's value hints, booleans.
    pub fn values(&self, key: &str) -> Vec<String> {
        if let Some(h) = self.hints(key) {
            return h.clone();
        }
        let boolean = match self.prop(key) {
            Some(p) => p.boolean(),
            None => match self.map_of(key) {
                Some(map) => {
                    if let Some(h) = self.hints(&format!("{}.values", map.name)) {
                        return h.clone();
                    }
                    map.ty.ends_with(",Boolean>")
                }
                None => false,
            },
        };
        if boolean { vec!["true".into(), "false".into()] } else { Vec::new() }
    }

    /// The map keys hinted for `map` (`root` for `logging.level`).
    pub fn map_keys(&self, map: &str) -> Vec<String> {
        self.hints(&format!("{map}.keys")).cloned().unwrap_or_default()
    }

    /// Every property, each once.
    pub fn all(&self) -> impl Iterator<Item = &Prop> {
        let mut seen = BTreeSet::new();
        self.0.iter().flat_map(|c| c.props.iter()).filter(move |(k, _)| seen.insert(k.as_str())).map(|(_, p)| p)
    }

    /// The written parent of `key` when it is not a property although that
    /// parent is a known group (`spring.datasource` for `spring.datasource.urll`).
    /// Keys below unknown groups, maps or list items are never reported.
    pub fn unknown<'k>(&self, key: &'k str) -> Option<&'k str> {
        if key.contains("${") || key.contains("#{") {
            return None;
        }
        let base = key.split('[').next().unwrap_or(key);
        let c = canonical(base);
        if self.prop(base).is_some() || self.group(&c) || self.map_of(base).is_some() {
            return None;
        }
        let (parent, _) = c.rsplit_once('.')?;
        let (written, _) = base.rsplit_once('.')?;
        (parent.contains('.') && self.group(parent)).then_some(written)
    }
}

fn documentation(p: &Prop) -> Option<String> {
    let mut doc = p.description.clone();
    if !p.default.is_empty() {
        doc += &format!("\n\nDefault: `{}`", p.default);
    }
    if let Some(dep) = &p.deprecated {
        doc += &match dep.replacement.as_str() {
            "" => "\n\nDeprecated.".to_owned(),
            r => format!("\n\nDeprecated: use `{r}`."),
        };
    }
    let doc = doc.trim();
    (!doc.is_empty()).then(|| doc.to_owned())
}

fn prop_item(label: &str, insert: String, p: Option<&Prop>, kind: CompletionKind, replace: Range) -> CompletionItem {
    let deprecated = p.is_some_and(|p| p.deprecated.is_some());
    CompletionItem {
        detail: p.map(|p| p.ty.clone()).filter(|t| !t.is_empty() && t != "*"),
        insert_text: insert,
        replace: Some(replace),
        sort_text: Some(format!("{}{label}", u8::from(deprecated))),
        documentation: p.and_then(documentation),
        ..CompletionItem::new(label, kind)
    }
}

fn value_items(lookup: &Lookup, key: &str, replace: Range) -> Vec<CompletionItem> {
    lookup
        .values(key)
        .into_iter()
        .map(|v| CompletionItem { replace: Some(replace), ..CompletionItem::new(&v, CompletionKind::Value) })
        .collect()
}

fn usable(p: &&Prop) -> bool {
    p.deprecated.as_ref().is_none_or(|d| !d.removed)
}

/// The line around byte `at`: its start and its end without trailing blanks.
fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (start, start + text[start..end].trim_end().len())
}

/// The key without its MicroProfile `%profile.` prefix.
pub fn strip_profile(key: &str) -> &str {
    if !key.starts_with('%') {
        return key;
    }
    key.split_once('.').map_or("", |(_, base)| base)
}

pub fn properties_completion(text: &str, at: usize, lookup: &Lookup) -> Vec<CompletionItem> {
    let (start, end) = line_bounds(text, at);
    let line = &text[start..end.max(at)];
    if line.trim_start().starts_with(['#', '!']) {
        return Vec::new();
    }
    let sep = line.find(['=', ':']).map(|i| start + i);
    if let Some(sep) = sep.filter(|&s| at > s) {
        let after = &text[sep + 1..end.max(sep + 1)];
        let value_at = (sep + 1 + after.len() - after.trim_start().len()).min(at);
        return value_items(lookup, strip_profile(text[start..sep].trim()), range(text, value_at, end.max(at)));
    }
    let mut key_start = (start + line.len() - line.trim_start().len()).min(at);
    if text[key_start..at].starts_with('%') {
        let Some(dot) = text[key_start..at].find('.') else { return Vec::new() };
        key_start += dot + 1;
    }
    let key_end = sep.map_or(end, |s| start + text[start..s].trim_end().len()).max(at);
    let typed = &text[key_start..at];
    let done = typed.rsplit_once('.').map(|(d, _)| canonical(d) + ".");
    let replace = range(text, key_start, key_end);
    let assign = if sep.is_some() { "" } else { "=" };
    let mut items = Vec::new();
    for p in lookup.all().filter(usable) {
        if done.as_ref().is_some_and(|d| !canonical(&p.name).starts_with(d.as_str())) {
            continue;
        }
        if p.open() {
            for k in lookup.map_keys(&p.name) {
                let name = format!("{}.{k}", p.name);
                items.push(prop_item(&name, format!("{name}{assign}"), Some(p), CompletionKind::Property, replace));
            }
            let name = format!("{}.", p.name);
            items.push(prop_item(&name, name.clone(), Some(p), CompletionKind::Module, replace));
        } else {
            items.push(prop_item(&p.name, format!("{}{assign}", p.name), Some(p), CompletionKind::Property, replace));
        }
    }
    items
}

/// Completion of a YAML key offers the next segment under the parent: a
/// property (`url: `) or a group to nest (`datasource:` and a new line).
pub fn yaml_completion(text: &str, at: usize, lookup: &Lookup) -> Vec<CompletionItem> {
    let (parent, typed) = yaml_parent(text, at);
    if typed.starts_with('#') {
        return Vec::new();
    }
    let (line_start, end) = line_bounds(text, at);
    let typed_at = at - typed.len();
    let join = |a: &str, b: &str| if a.is_empty() || b.is_empty() { format!("{a}{b}") } else { format!("{a}.{b}") };
    if let Some(colon) = typed.find(": ") {
        let value = &typed[colon + 1..];
        let value_at = typed_at + colon + 1 + value.len() - value.trim_start().len();
        let key = join(&parent, typed[..colon].trim().trim_matches(['"', '\'']));
        let key = strip_profile(&key);
        return value_items(lookup, &key, range(text, value_at, end.max(at)));
    }
    let typed_done = typed.rfind('.').map_or("", |d| &typed[..=d]);
    let done = join(&parent, typed_done.trim_end_matches('.'));
    let done = strip_profile(&done).to_owned();
    let depth = if done.is_empty() { 0 } else { done.split('.').count() };
    let prefix = if done.is_empty() { String::new() } else { canonical(&done) + "." };
    let rest = &text[at..end.max(at)];
    let word_end = at + rest.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(rest.len());
    let has_colon = text[word_end..end.max(word_end)].trim_start().starts_with(':');
    let nested = format!(":\n{}", " ".repeat(typed_at - line_start + 2));
    let replace = range(text, typed_at, word_end);

    // Segment → (property when it is one, properties below it).
    let mut found: BTreeMap<String, (Option<&Prop>, usize)> = BTreeMap::new();
    if let Some(map) = lookup.prop(&done).filter(|p| p.open()) {
        for k in lookup.map_keys(&map.name) {
            found.entry(k).or_insert((Some(map), 0));
        }
    }
    for p in lookup.all().filter(usable) {
        if !canonical(&p.name).starts_with(&prefix) {
            continue;
        }
        let segments: Vec<&str> = p.name.split('.').collect();
        let Some(seg) = segments.get(depth) else { continue };
        let entry = found.entry((*seg).to_owned()).or_insert((None, 0));
        if segments.len() == depth + 1 {
            entry.0 = Some(p);
        } else {
            entry.1 += 1;
        }
    }
    found
        .into_iter()
        .map(|(seg, (prop, below))| {
            let map_key = prop.is_some_and(|p| canonical(&p.name) == canonical(&done));
            let leaf = prop.filter(|p| below == 0 && (map_key || !p.open()));
            let suffix = match (has_colon, leaf) {
                (true, _) => "",
                (false, Some(_)) => ": ",
                (false, None) => &nested,
            };
            let kind = if leaf.is_some() { CompletionKind::Property } else { CompletionKind::Module };
            let mut item = prop_item(&seg, format!("{typed_done}{seg}{suffix}"), prop, kind, replace);
            item.filter_text = Some(format!("{typed_done}{seg}"));
            if prop.is_none() {
                item.detail = Some(format!("{below} properties"));
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::providers::CompletionItem;

    const TSV: &str = "# test\nacme.http.port\tint\t8080\t\tThe port\nacme.http.mode\tMode\t\t\t\nacme.old\tString\t\tw:\tGone\n@acme.http.mode\tfast\tsafe\n";

    fn labels(items: Vec<CompletionItem>) -> Vec<String> {
        items.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn strips_profiles() {
        assert_eq!(strip_profile("%dev.quarkus.http.port"), "quarkus.http.port");
        assert_eq!(strip_profile("%dev"), "");
        assert_eq!(strip_profile("quarkus.http.port"), "quarkus.http.port");
    }

    #[test]
    fn completes_after_a_profile() {
        let mut catalog = Catalog::default();
        catalog.add(&parse_tsv(TSV));
        let lookup = Lookup(vec![&catalog]);
        let text = "%dev.acme.http.po";
        let items = properties_completion(text, text.len(), &lookup);
        let port = items.iter().find(|i| i.label == "acme.http.port").unwrap();
        assert_eq!(port.insert_text, "acme.http.port=");
        assert_eq!(port.replace, Some(crate::range(text, 5, text.len())));
        assert!(properties_completion("%de", 3, &lookup).is_empty());
        assert_eq!(labels(properties_completion("%test.acme.http.mode=", 21, &lookup)), ["fast", "safe"]);
        let yaml = "\"%dev\":\n  acme:\n    http:\n      ";
        assert_eq!(labels(yaml_completion(yaml, yaml.len(), &lookup)), ["mode", "port"]);
        let yaml = "\"%prod\":\n  acme:\n    http:\n      mode: ";
        assert_eq!(labels(yaml_completion(yaml, yaml.len(), &lookup)), ["fast", "safe"]);
    }
}
