//! Read-only physical source occurrence review. Never publishes automatic edits.
use crate::*;
use std::time::{Duration, Instant};
const MAX_FILES: usize = 500;
const MAX_BYTES: usize = 128 << 10;
#[derive(Debug)]
struct Occurrence {
    path: String,
    text: std::rc::Rc<String>,
    range: Range,
}
fn owned(project: &Project, root: &str, path: &str) -> bool {
    crate::file_actions::normalized(path)
        && under(path, root)
        && host::project_of(path)
            .and_then(|p| p.root().ok())
            .as_deref()
            == Some(root)
        && project
            .stat(path)
            .is_ok_and(|s| s.kind == openidea_plugin::FileKind::File && s.size <= MAX_BYTES as u64)
}
fn occurrences(doc: &Document, kind: Kind, name: &str) -> Vec<Range> {
    if doc.text.len() > MAX_BYTES {
        return vec![];
    }
    refs::references(&doc.language, &doc.text)
        .into_iter()
        .filter_map(|r| {
            if r.kind != kind || &doc.text[r.start..r.end] != name {
                return None;
            }
            let tail = doc.text[r.end + 1..].trim_start();
            if tail.starts_with(['~', '.', '?', '+', '-', '|']) {
                return None;
            }
            let proven = ownership::reference(doc, r.start)?;
            (proven.kind == kind && proven.start == r.start && proven.end == r.end)
                .then(|| range_of(&doc.text, r.start, r.end))
        })
        .collect()
}
pub(crate) fn run(owner: &Main, ctx: &Invocation) -> Result<Option<String>, Error> {
    let editor = ctx.editor().ok_or_else(|| {
        Error::Invalid("Open a quoted Symfony route, service or template reference first.".into())
    })?;
    let snapshot = editor.document()?;
    let origin = Document {
        path: editor.path(),
        language: editor.language(),
        text: snapshot.text,
        version: snapshot.version,
    };
    let project = host::project_of(&origin.path).ok_or(Error::Stale)?;
    let root = project.root()?;
    if ctx.project().and_then(|p| p.root().ok()).as_deref() != Some(root.as_str()) {
        return Err(Error::Stale);
    }
    let at = editor
        .selections()?
        .first()
        .copied()
        .ok_or(Error::Stale)?
        .start;
    let offset = text::offset(&origin.text, at).ok_or(Error::Stale)?;
    let reference=ownership::reference(&origin,offset).filter(|r|matches!(r.kind,Kind::Route|Kind::Service|Kind::Template))
        .ok_or_else(||Error::Invalid("Put the caret on a route, service or template name written as a plain string. Names built at runtime are not supported.".into()))?;
    let index = owner.index_for(&origin.path).ok_or(Error::Stale)?;
    let name = &origin.text[reference.start..reference.end];
    if name.len() > 512 {
        return Err(Error::Invalid(
            "The source reference name exceeds 512 bytes.".into(),
        ));
    }
    let entry = match reference.kind {
        Kind::Route => index.routes.get(name),
        Kind::Service => index.services.get(name),
        Kind::Template => {
            if index.ambiguous_templates.contains(name) {
                None
            } else {
                index.templates.get(name)
            }
        }
        _ => None,
    }
    .ok_or_else(|| {
        Error::NotFound("No declaration of this name was found. Run Symfony: Reindex Project, then try again.".into())
    })?;
    if !owned(&project, &root, &entry.path) {
        return Err(Error::Stale);
    }
    // A task gives the call 12 times the fuel of a plain call, so the 3 s scan can't trap the plugin.
    let _task = host::begin_task("Searching Symfony sources", false);
    let started = Instant::now();
    let mut partial = index.partial;
    let mut paths = BTreeSet::new();
    for glob in [
        "**/*.php",
        "**/*.twig",
        "config/**/*.yaml",
        "config/**/*.yml",
    ] {
        match project.find_files(glob, (MAX_FILES + 1) as u32) {
            Ok(found) => {
                partial |= found.len() > MAX_FILES;
                paths.extend(found);
            }
            Err(_) => partial = true,
        }
    }
    paths.insert(origin.path.clone());
    let mut bytes = 0usize;
    let mut found = vec![];
    for (number, path) in paths
        .into_iter()
        .filter(|p| {
            !p.split('/')
                .any(|c| matches!(c, "vendor" | "node_modules" | ".git"))
                && !p.contains("/var/cache/")
        })
        .enumerate()
    {
        if number >= MAX_FILES || bytes >= 8 << 20 || started.elapsed() > Duration::from_secs(3) {
            partial = true;
            break;
        }
        let Some(language) = language_of(&path) else {
            continue;
        };
        if !owned(&project, &root, &path) {
            partial = true;
            continue;
        }
        let source = if path == origin.path {
            origin.text.clone()
        } else {
            match project.read_file(&path) {
                Ok(s) => s,
                Err(_) => {
                    partial = true;
                    continue;
                }
            }
        };
        bytes += source.len();
        if source.len() > MAX_BYTES {
            partial = true;
            continue;
        }
        let doc = Document {
            path: path.clone(),
            language: language.into(),
            text: source,
            version: 0,
        };
        let source = std::rc::Rc::new(doc.text.clone());
        for range in occurrences(&doc, reference.kind, name) {
            if found.len() >= 512 {
                partial = true;
                break;
            }
            found.push(Occurrence {
                path: path.clone(),
                text: source.clone(),
                range,
            });
        }
    }
    let items: Vec<_> = found
        .iter()
        .enumerate()
        .map(|(i, r)| {
            PickItem::new(
                &i.to_string(),
                &format!(
                    "{}:{}:{}",
                    r.path.strip_prefix(&format!("{root}/")).unwrap_or(&r.path),
                    r.range.start.line + 1,
                    r.range.start.character + 1
                ),
            )
        })
        .collect();
    if items.is_empty() {
        return Ok(Some(
            "No usages found in the project sources. Names built at runtime are not searched.".into(),
        ));
    }
    let title = format!(
        "{}{} usages of {name} in project sources",
        if partial { "Partial: " } else { "" },
        items.len()
    );
    let Some(key) = host::pick(&title, &items)? else {
        return Ok(None);
    };
    let result = key
        .parse::<usize>()
        .ok()
        .and_then(|i| found.get(i))
        .ok_or(Error::Stale)?;
    if editor.document()?.text != origin.text
        || editor.document()?.version != origin.version
        || project.root()? != root
        || !owned(&project, &root, &result.path)
    {
        return Err(Error::Stale);
    }
    let fresh = project.read_file(&result.path)?;
    if fresh != result.text.as_str() {
        return Err(Error::Stale);
    }
    host::open_location(
        &result.path,
        result.range.start.line,
        result.range.start.character,
    )?;
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document {
            path: path.into(),
            language: language.into(),
            text: text.into(),
            version: 1,
        }
    }
    #[test]
    fn source_occurrences_exclude_comments_raw_dynamic_and_other_families() {
        let source = "😀 {{ path('blog') }} {# {{ path('blog') }} #} {% verbatim %}{{ path('blog') }}{% endverbatim %} {{ path('blog' ~ extra) }} {{ service('blog') }}";
        let d = doc("/project/templates/a.twig", "twig", source);
        let found = occurrences(&d, Kind::Route, "blog");
        assert_eq!(found.len(), 1);
        assert_eq!(
            text::offset(source, found[0].start).unwrap(),
            source.find("'blog'").unwrap() + 1
        );
        let p = doc(
            "/project/src/A.php",
            "php",
            "<?php // $this->generateUrl('blog');\n$this->generateUrl('blog'); $x=\"$this->generateUrl('blog')\";",
        );
        assert_eq!(occurrences(&p, Kind::Route, "blog").len(), 1);
    }
    #[test]
    fn command_uses_current_editor_and_navigates_exact_scanned_source() {
        let host = openidea_plugin::mock::MockHost::install().with_capabilities(&[
            "project.read",
            "editor.read",
            "storage",
        ]);
        let source = "{{ path('blog') }}";
        host.add_file("templates/a.twig", source);
        host.add_file("templates/b.twig", "😀 {{ path('blog') }}");
        host.add_file("config/routes.yaml", "blog:\n  path: /blog\n");
        host.open_editor(
            "/project/templates/a.twig",
            "twig",
            source,
            &[(0, 10, 0, 10)],
        );
        let mut owner = Main::default();
        let mut index = Index::default();
        index.routes.insert(
            "blog".into(),
            Entry {
                path: "/project/config/routes.yaml".into(),
                detail: None,
                at: Position::new(0, 0),
            },
        );
        owner.indexes.insert("/project".into(), index);
        host.answer_next_prompt(Some(&["1"]));
        run(&owner, &host.invocation()).unwrap();
        assert_eq!(
            host.opened_locations(),
            [("/project/templates/b.twig".into(), 0, 12)]
        );
        assert!(host.process_runs().is_empty());
    }
}
