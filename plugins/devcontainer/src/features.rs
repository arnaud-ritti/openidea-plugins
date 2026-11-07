//! Feature ids for `features` keys, from the public containers.dev index. The index is fetched by a command only and kept
//! in plugin storage; completion reads the stored copy.

use openidea_plugin::host::{self, storage};
use openidea_plugin::{Error, HttpRequest, NotificationLevel};

pub const INDEX_URL: &str = "https://containers.dev/static/devcontainer-index.json";
const CACHE_KEY: &str = "feature-index";
const MAX_CACHE_BYTES: usize = 900 * 1024;

/// One `reference<TAB>name` line per feature, the reference written as a `features` key (`ghcr.io/x/features/node:1`).
pub fn parse_index(body: &str) -> Option<String> {
    let index: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut lines = String::new();
    for collection in index.get("collections")?.as_array()? {
        for feature in collection.get("features").and_then(|f| f.as_array()).into_iter().flatten() {
            let Some(id) = feature.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let reference = match feature.get("majorVersion").and_then(|v| v.as_str()) {
                Some(major) => format!("{id}:{major}"),
                None => id.to_owned(),
            };
            let name = feature.get("name").and_then(|v| v.as_str()).unwrap_or_default().replace(['\t', '\n'], " ");
            let line = format!("{reference}\t{name}\n");
            if lines.len() + line.len() > MAX_CACHE_BYTES {
                break;
            }
            lines.push_str(&line);
        }
    }

    (!lines.is_empty()).then_some(lines)
}

pub fn cached() -> Vec<(String, String)> {
    let Ok(Some(lines)) = storage::get(CACHE_KEY) else {
        return Vec::new();
    };

    lines
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(reference, name)| (reference.to_owned(), name.to_owned()))
        .collect()
}

pub fn refresh() -> Result<Option<String>, Error> {
    let response = host::fetch(&HttpRequest::get(INDEX_URL))?;
    if response.status != 200 {
        return Err(Error::Failed(format!("containers.dev answered {}.", response.status)));
    }
    let lines = parse_index(&response.text()).ok_or("The containers.dev index has an unexpected shape.")?;
    storage::set(CACHE_KEY, &lines)?;
    host::notify(NotificationLevel::Info, "Dev Container", &format!("Loaded {} features.", lines.lines().count()));

    Ok(None)
}
