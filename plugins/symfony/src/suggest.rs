//! "Did you mean" for names that are close to exactly one known name.

const MAX_LENGTH: usize = 64;

/// The edit distance of two names, or `None` once it exceeds `limit`.
fn distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }

    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, left) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, right) in b.iter().enumerate() {
            let cost = usize::from(left != right);
            current.push((previous[j] + cost).min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }

    Some(previous[b.len()]).filter(|found| *found <= limit)
}

/// The single known name nearest to `name` within one edit for short names
/// and two for longer ones; `None` when none is near or two tie.
pub fn closest<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    if name.chars().count() > MAX_LENGTH {
        return None;
    }

    let limit = if name.chars().count() <= 4 { 1 } else { 2 };
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;

    for candidate in known.filter(|candidate| *candidate != name) {
        let Some(found) = distance(name, candidate, limit) else { continue };
        match best {
            Some((nearest, _)) if found > nearest => {}
            Some((nearest, _)) if found == nearest => tied = true,
            _ => {
                best = Some((found, candidate));
                tied = false;
            }
        }
    }

    best.filter(|_| !tied).map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::closest;

    #[test]
    fn picks_one_near_name() {
        let known = ["blog_show", "blog_index", "app_home"];
        assert_eq!(closest("blog_shw", known.into_iter()), Some("blog_show"));
        assert_eq!(closest("blgo_show", known.into_iter()), Some("blog_show"));
        assert_eq!(closest("app_hom", known.into_iter()), Some("app_home"));
    }

    #[test]
    fn stays_silent_for_ties_far_names_and_exact_matches() {
        assert_eq!(closest("blog_sho", ["blog_show", "blog_shoe"].into_iter()), None);
        assert_eq!(closest("xyz_page", ["blog_show"].into_iter()), None);
        assert_eq!(closest("blog_show", ["blog_show"].into_iter()), None);
        assert_eq!(closest("ab", ["xyz"].into_iter()), None);
        assert_eq!(closest(&"a".repeat(70), ["a"].into_iter()), None);
    }

    #[test]
    fn short_names_allow_one_edit_and_counts_characters() {
        assert_eq!(closest("hom", ["home", "xhome"].into_iter()), Some("home"));
        assert_eq!(closest("hm", ["home"].into_iter()), None);
        assert_eq!(closest("café_x", ["cafe_x"].into_iter()), Some("cafe_x"));
    }
}
