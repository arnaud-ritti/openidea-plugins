//! Checks for values from form fields, the repository or the server before
//! they reach a `git` argument or a REST path. A valid value comes back unchanged.

use openidea_plugin::Error;

pub const EMOJIS: [&str; 8] = ["thumbsup", "thumbsdown", "heart", "tada", "laughing", "confused", "rocket", "eyes"];

const GITLAB: &str = "https://gitlab.com/";

fn refused(what: &str, s: &str) -> Error {
    let shown: String = s.chars().take(40).collect();

    Error::Invalid(format!("refusing the {what} {shown:?}"))
}

fn accept<'a>(what: &str, s: &'a str, valid: bool) -> Result<&'a str, Error> {
    match valid {
        true => Ok(s),
        false => Err(refused(what, s)),
    }
}

fn charset(s: &str, max: usize, extra: &[u8]) -> bool {
    (1..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

pub fn ref_name(s: &str) -> Result<&str, Error> {
    let valid = charset(s, 200, b"._/@+-") && !s.starts_with(['-', '/']) && !s.contains("..") && !s.ends_with('/') && !s.ends_with(".lock");

    accept("branch name", s, valid)
}

pub fn remote_name(s: &str) -> Result<&str, Error> {
    accept("name", s, charset(s, 64, b"_.-") && !s.starts_with(['-', '.']))
}

pub fn hex_sha(s: &str) -> Result<&str, Error> {
    accept("commit", s, (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn discussion_id(s: &str) -> Result<&str, Error> {
    accept("thread id", s, (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn emoji(s: &str) -> Result<&'static str, Error> {
    EMOJIS.into_iter().find(|e| *e == s).ok_or_else(|| refused("emoji", s))
}

pub fn gitlab_url(s: &str) -> Result<&str, Error> {
    accept("URL", s, s.starts_with(GITLAB) && !s.chars().any(|c| c.is_whitespace() || c.is_control()))
}

pub fn repo_file_path(s: &str) -> Result<&str, Error> {
    let valid = !s.contains(['\\', '\0']) && s.split('/').all(|segment| !["", ".", ".."].contains(&segment));

    accept("file path", s, valid)
}

/// A gitlab.com clone URL (HTTPS or SSH) that is safe as a terminal argument, shell-joined or not.
pub fn clone_url(s: &str) -> Result<&str, Error> {
    let path = s.strip_prefix(GITLAB).or_else(|| s.strip_prefix("git@gitlab.com:")).unwrap_or("");

    accept("clone URL", s, charset(path, 1000, b"_./-") && repo_file_path(path).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(check: fn(&str) -> Result<&str, Error>, values: &[&str]) {
        for value in values {
            assert!(check(value).is_err(), "{value:?} was accepted");
        }
    }

    #[test]
    fn ref_name_refuses_options_traversal_and_metacharacters() {
        refused(ref_name, &["-x", "--upload-pack=x", "a/../b", "a;rm -rf ~", "a b", "$(x)", "x.lock", "", "/a", "a/", "a\nb"]);
        refused(ref_name, &[&"a".repeat(201)]);

        assert_eq!(ref_name("feature/a-1").unwrap(), "feature/a-1");
        assert_eq!(ref_name("v1.2+build@x").unwrap(), "v1.2+build@x");
    }

    #[test]
    fn remote_name_refuses_leading_dash_and_dot() {
        refused(remote_name, &["-x", ".x", "", "a/b", "a b", &"a".repeat(65)]);

        assert_eq!(remote_name("my_tool.rs-2").unwrap(), "my_tool.rs-2");
    }

    #[test]
    fn hex_sha_requires_7_to_40_hex() {
        refused(hex_sha, &["abcdef", "abcdefg", "--output=/tmp/x", &"a".repeat(41)]);

        assert_eq!(hex_sha("abcdef0").unwrap(), "abcdef0");
        assert!(hex_sha(&"F".repeat(40)).is_ok());
    }

    #[test]
    fn discussion_id_refuses_slashes_and_queries() {
        refused(discussion_id, &["abc/notes", "abc?x=1", "", "..", &"a".repeat(65)]);

        assert_eq!(discussion_id("d1").unwrap(), "d1");
        assert!(discussion_id(&"0".repeat(64)).is_ok());
    }

    #[test]
    fn emoji_must_be_listed() {
        assert!(emoji("thumbsup/../x").is_err());
        assert!(emoji("Thumbsup").is_err());

        assert_eq!(emoji("rocket").unwrap(), "rocket");
    }

    #[test]
    fn gitlab_url_refuses_other_hosts_and_whitespace() {
        refused(gitlab_url, &["https://gitlab.com.evil.test/x", "http://gitlab.com/x", "https://gitlab.com/x y", "https://gitlab.com/x\n"]);

        assert_eq!(gitlab_url("https://gitlab.com/g/p/-/merge_requests/7").unwrap(), "https://gitlab.com/g/p/-/merge_requests/7");
    }

    #[test]
    fn repo_file_path_refuses_traversal() {
        refused(repo_file_path, &["../x", "a/../../x", "/etc/passwd", "a\\b", "a//b", "./a", "a/", "", "a\0b"]);

        assert_eq!(repo_file_path("src/a b.rs").unwrap(), "src/a b.rs");
    }

    #[test]
    fn clone_url_refuses_other_hosts_and_shell_metacharacters() {
        refused(clone_url, &["https://evil.test/me/tool.git", "https://gitlab.com/me/tool.git;rm -rf ~", "git@gitlab.com:../x.git", "git@gitlab.com:$(x).git"]);

        assert!(clone_url("https://gitlab.com/me/tool.git").is_ok());
        assert!(clone_url("git@gitlab.com:me/tool.git").is_ok());
    }

    #[test]
    fn a_refusal_echoes_at_most_40_characters_of_the_value() {
        let long = format!("-{}", "y".repeat(100));

        let message = ref_name(&long).unwrap_err().to_string();

        assert!(message.contains(&"y".repeat(39)) && !message.contains(&"y".repeat(40)), "{message}");
    }
}
