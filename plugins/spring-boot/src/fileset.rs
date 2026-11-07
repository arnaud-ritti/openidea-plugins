//! Configuration files that are not named `application*` or `bootstrap*`: the ones
//! `spring.config.import`, `spring.config.location`, `spring.config.additional-location`
//! and `spring.config.name` of the standard files point at.

use spring_text::config::{self, canonical, is_config_file};

pub const GLOBS: &[&str] = &[
    "**/application*.properties",
    "**/application*.yml",
    "**/application*.yaml",
    "**/bootstrap*.properties",
    "**/bootstrap*.yml",
    "**/bootstrap*.yaml",
];

const LOCATION_KEYS: &[&str] = &["spring.config.import", "spring.config.location", "spring.config.additional-location"];
const EXTENSIONS: &[&str] = &["properties", "yml", "yaml"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locations {
    /// Paths that end a file's path (`config/extra.yml`).
    pub tails: Vec<String>,
    /// Base names (`myapp` for `myapp.yml` and `myapp-dev.yml`).
    pub names: Vec<String>,
}

fn tail_of(location: &str) -> Option<String> {
    let mut one = location.trim().trim_matches(['"', '\'']);
    one = one.strip_prefix("optional:").unwrap_or(one);
    for scheme in ["classpath*:", "classpath:", "file:"] {
        one = one.strip_prefix(scheme).unwrap_or(one);
    }
    if one.contains([':', '*', '$']) {
        return None;
    }
    let one = one.trim_start_matches("./").trim_start_matches('/');
    let extension = one.rsplit_once('.')?.1;
    EXTENSIONS.contains(&extension).then(|| one.to_owned())
}

pub fn read(path: &str, text: &str) -> Option<Locations> {
    if !is_config_file(path) {
        return None;
    }
    let mut found = Locations::default();
    for key in config::keys(path, text) {
        let base = key.key.split('[').next().unwrap_or(&key.key);
        let base = canonical(base);
        if LOCATION_KEYS.contains(&base.as_str()) {
            found.tails.extend(key.value.split([',', ';']).filter_map(tail_of));
        } else if base == "spring.config.name" {
            found.names.extend(key.value.split(',').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty() && !n.contains("${")));
        }
    }
    (!found.tails.is_empty() || !found.names.is_empty()).then_some(found)
}

/// Whether Spring Boot reads `path` as configuration, given what the standard files point at.
pub fn is_configuration<'a>(path: &str, sets: impl Iterator<Item = &'a Locations>) -> bool {
    if is_config_file(path) {
        return true;
    }
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((stem, extension)) = file.rsplit_once('.') else { return false };
    if !EXTENSIONS.contains(&extension) {
        return false;
    }
    let normalized = path.replace('\\', "/");
    sets.into_iter().any(|set| {
        set.tails.iter().any(|t| normalized == *t || normalized.ends_with(&format!("/{t}")))
            || set.names.iter().any(|n| stem == n || stem.strip_prefix(n.as_str()).is_some_and(|rest| rest.starts_with('-')))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_imports_locations_and_names() {
        let text = "spring.config.import=optional:classpath:extra.yml,file:./conf/more.properties,configtree:/run/x/,vault://s\nspring.config.name=myapp\n";
        let set = read("application.properties", text).unwrap();
        assert_eq!(set.tails, ["extra.yml", "conf/more.properties"]);
        assert_eq!(set.names, ["myapp"]);
        let sets = [set];
        assert!(is_configuration("/p/src/main/resources/extra.yml", sets.iter()));
        assert!(is_configuration("/p/conf/more.properties", sets.iter()));
        assert!(is_configuration("/p/myapp-dev.yaml", sets.iter()));
        assert!(!is_configuration("/p/other.yml", sets.iter()));
        assert!(is_configuration("/p/application.yml", [].iter()));
        assert_eq!(read("custom.yml", text), None);
    }

    #[test]
    fn reads_yaml_sequences() {
        let text = "spring:\n  config:\n    import:\n      - classpath:a.yml\n      - optional:b.yml\n";
        assert_eq!(read("application.yml", text).unwrap().tails, ["a.yml", "b.yml"]);
    }
}
