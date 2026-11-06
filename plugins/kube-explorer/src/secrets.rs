//! Secret handling: decoding `data` from `kubectl get secret -o json`, and
//! building the manifest of a new Opaque Secret. Values only ever live in
//! these functions' arguments and results; nothing here logs or stores them.

use openidea_plugin::views::{Body, Button, Field, FieldValue, View};
use openidea_plugin::{Error, host};

use crate::{KUBECTL, KubeExplorer, REQUEST_TIMEOUT, TREE, config, failure};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        bits = bits << 6 | ALPHABET.iter().position(|a| *a == c)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    Some(out)
}

/// Text when the bytes are UTF-8, otherwise their base64.
pub fn display(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => format!("(binary, base64) {}", b64_encode(bytes)),
    }
}

struct Json<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Json<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.peek() != Some(c) {
            return Err(format!("unexpected JSON at byte {}", self.at));
        }
        self.at += 1;
        Ok(())
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = *self.bytes.get(self.at).ok_or("unterminated JSON string")?;
            self.at += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let e = *self.bytes.get(self.at).ok_or("unterminated JSON string")?;
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self.bytes.get(self.at..self.at + 4).and_then(|h| std::str::from_utf8(h).ok());
                            let code = hex.and_then(|h| u32::from_str_radix(h, 16).ok());
                            self.at += 4;
                            let ch = code.and_then(char::from_u32).unwrap_or('\u{fffd}');
                            out.extend(ch.to_string().bytes());
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn skip(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(b'"') => self.string().map(drop),
            Some(open @ (b'{' | b'[')) => {
                let close = if open == b'{' { b'}' } else { b']' };
                self.at += 1;
                while self.peek() != Some(close) {
                    if open == b'{' {
                        self.string()?;
                        self.eat(b':')?;
                    }
                    self.skip()?;
                    if self.peek() == Some(b',') {
                        self.at += 1;
                    }
                }
                self.at += 1;
                Ok(())
            }
            Some(_) => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|c| !b",}] \n\r\t".contains(c)) {
                    self.at += 1;
                }
                if self.at == start {
                    return Err(format!("unexpected JSON at byte {start}"));
                }
                Ok(())
            }
            None => Err("truncated JSON".into()),
        }
    }
}

/// The decoded top-level `data` of a Secret, in the order kubectl printed it.
pub fn data_entries(json: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut p = Json { bytes: json.as_bytes(), at: 0 };
    let mut entries = Vec::new();
    p.eat(b'{')?;
    while p.peek() != Some(b'}') {
        let key = p.string()?;
        p.eat(b':')?;
        if key != "data" || p.peek() != Some(b'{') {
            p.skip()?;
        } else {
            p.eat(b'{')?;
            while p.peek() != Some(b'}') {
                let name = p.string()?;
                p.eat(b':')?;
                let value = p.string()?;
                let bytes = b64_decode(&value).ok_or_else(|| format!("the value of {name} is not valid base64"))?;
                entries.push((name, bytes));
                if p.peek() == Some(b',') {
                    p.at += 1;
                }
            }
            p.at += 1;
        }
        if p.peek() == Some(b',') {
            p.at += 1;
        }
    }
    Ok(entries)
}

/// A DNS subdomain: what Kubernetes accepts as a Secret name.
pub fn valid_name(name: &str) -> bool {
    let edge = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    name.len() <= 253
        && name.chars().next().is_some_and(edge)
        && name.chars().last().is_some_and(edge)
        && name.chars().all(|c| edge(c) || c == '-' || c == '.')
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 253
        && key != "."
        && key != ".."
        && key.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
}

/// An Opaque Secret manifest. Names and keys are restricted to characters that
/// need no escaping and values are base64, so any value round-trips.
pub fn manifest(name: &str, entries: &[(String, String)]) -> Result<String, String> {
    if !valid_name(name) {
        return Err("The Secret name must be lowercase letters, digits, '-' and '.', starting and ending with a letter or digit.".into());
    }
    if entries.is_empty() {
        return Err("Add at least one key.".into());
    }
    let mut out = format!("apiVersion: v1\nkind: Secret\nmetadata:\n  name: \"{name}\"\ntype: Opaque\ndata:\n");
    for (i, (key, value)) in entries.iter().enumerate() {
        if !valid_key(key) {
            return Err("A key is invalid: use letters, digits, '-', '_' and '.' only.".into());
        }
        if entries[..i].iter().any(|(k, _)| k == key) {
            return Err(format!("The key \"{key}\" is given twice."));
        }
        out.push_str(&format!("  \"{key}\": {}\n", b64_encode(value.as_bytes())));
    }
    Ok(out)
}

/// `text` without the values (raw or base64), for messages.
pub fn redact(text: &str, entries: &[(String, String)]) -> String {
    let mut text = text.to_owned();
    for (_, value) in entries.iter().filter(|(_, v)| !v.is_empty()) {
        text = text.replace(&b64_encode(value.as_bytes()), "***").replace(value.as_str(), "***");
    }
    text
}

/// Byte span of the value of a top-level member of a JSON object.
fn member_span(json: &str, name: &str) -> Result<Option<(usize, usize)>, String> {
    let mut p = Json { bytes: json.as_bytes(), at: 0 };
    p.eat(b'{')?;
    while p.peek() != Some(b'}') {
        let key = p.string()?;
        p.eat(b':')?;
        p.peek();
        let start = p.at;
        p.skip()?;
        if key == name {
            return Ok(Some((start, p.at)));
        }
        if p.peek() == Some(b',') {
            p.at += 1;
        }
    }
    Ok(None)
}

/// `json` (a whole Secret) with its `data` replaced; everything else, the resourceVersion included, stays.
pub fn replace_data(json: &str, data: &[(String, Vec<u8>)]) -> Result<String, String> {
    let members: Vec<String> = data.iter().map(|(key, bytes)| format!("\"{key}\":\"{}\"", b64_encode(bytes))).collect();
    let object = format!("{{{}}}", members.join(","));
    match member_span(json, "data")? {
        Some((start, end)) => Ok(format!("{}{object}{}", &json[..start], &json[end..])),
        None => {
            let open = json.find('{').ok_or("the Secret is not a JSON object")?;
            Ok(format!("{}\"data\":{object},{}", &json[..=open], &json[open + 1..]))
        }
    }
}

const FIELD_PREFIX: &str = "secret-edit:";

/// A Secret open for editing: its object as read, so a save replaces it only if nobody changed it since.
pub struct SecretEdit {
    row: String,
    json: String,
    entries: Vec<(String, String)>,
    binary: Vec<(String, Vec<u8>)>,
    new_key: String,
    new_value: String,
    remove: String,
}

impl SecretEdit {
    pub fn set_field(&mut self, key: &str, value: String) {
        match key {
            "secret-new-key" => self.new_key = value,
            "secret-new-value" => self.new_value = value,
            "secret-remove" => self.remove = value,
            _ => {
                if let Some(entry) = key.strip_prefix(FIELD_PREFIX).and_then(|k| self.entries.iter_mut().find(|(name, _)| name == k)) {
                    entry.1 = value;
                }
            }
        }
    }

    fn data(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut entries = self.entries.clone();
        if !self.new_key.is_empty() || !self.new_value.is_empty() {
            entries.push((self.new_key.clone(), self.new_value.clone()));
        }
        let removed: Vec<&str> = self.remove.split_whitespace().collect();
        entries.retain(|(key, _)| !removed.contains(&key.as_str()));
        let mut data: Vec<(String, Vec<u8>)> = self.binary.iter().filter(|(key, _)| !removed.contains(&key.as_str())).cloned().collect();
        for (key, value) in entries {
            if !valid_key(&key) {
                return Err("A key is invalid: use letters, digits, '-', '_' and '.' only.".into());
            }
            if data.iter().any(|(existing, _)| *existing == key) {
                return Err(format!("The key \"{key}\" is given twice."));
            }
            data.push((key, value.into_bytes()));
        }
        Ok(data)
    }
}

impl KubeExplorer {
    pub fn edit_secret(&mut self, row: &str) -> Result<(), Error> {
        if !Self::secret_gate("secrets") {
            return Ok(());
        }
        let (namespace, name) = self.split_scope("secrets", row)?;
        let json = self.kubectl_in(namespace.as_deref(), &["get", "secret", &name, "-o", "json", REQUEST_TIMEOUT], None)?;
        let mut entries = Vec::new();
        let mut binary = Vec::new();
        for (key, bytes) in data_entries(&json)? {
            match String::from_utf8(bytes) {
                Ok(text) => entries.push((key, text)),
                Err(error) => binary.push((key, error.into_bytes())),
            }
        }
        self.editing_secret = Some(SecretEdit {
            row: row.to_owned(),
            json,
            entries,
            binary,
            new_key: String::new(),
            new_value: String::new(),
            remove: String::new(),
        });
        self.details = None;
        host::refresh_view(TREE);
        Ok(())
    }

    pub fn render_secret_edit(&self, edit: &SecretEdit) -> View {
        let mut fields: Vec<Field> = edit
            .entries
            .iter()
            .map(|(key, value)| Field::new(&format!("{FIELD_PREFIX}{key}"), key, FieldValue::Multiline(value.clone())))
            .collect();
        fields.push(Field::new("secret-new-key", "New key", FieldValue::Text(edit.new_key.clone())));
        fields.push(Field::new("secret-new-value", "New value", FieldValue::Multiline(edit.new_value.clone())));
        fields.push(Field::new("secret-remove", "Keys to remove (separated by spaces)", FieldValue::Text(edit.remove.clone())));
        let binary = match edit.binary.is_empty() {
            true => String::new(),
            false => format!("; binary values kept unchanged: {}", edit.binary.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")),
        };
        View::new(Body::Form(fields))
            .status(&format!("Editing secret {}{binary}; values are shown here only", edit.row))
            .button(Button::new("back", "Back").icon("arrow-left"))
            .button(Button::new("submit-secret-edit", "Save").icon("check"))
    }

    pub fn save_secret_edit(&mut self) -> Result<(), Error> {
        let Some(edit) = &self.editing_secret else { return Ok(()) };
        if !Self::reveal_allowed() {
            self.editing_secret = None;
            return Ok(());
        }
        let data = edit.data()?;
        let (namespace, _) = self.split_scope("secrets", &edit.row)?;
        let manifest = replace_data(&edit.json, &data)?;
        let shown: Vec<(String, String)> = data.iter().map(|(k, v)| (k.clone(), String::from_utf8_lossy(v).into_owned())).collect();
        let args = self.args_in(namespace.as_deref(), &["replace", "-f", "-"]);
        let out = host::run_process(KUBECTL, &args, Some(&manifest))?;
        if out.status != Some(0) {
            let stderr = redact(&out.stderr, &shown);
            if stderr.contains("Conflict") || stderr.contains("has been modified") {
                return Err("The secret changed in the cluster since it was opened. Click Back and Edit Secret again; nothing was saved.".into());
            }
            return Err(failure(KUBECTL, &stderr, out.status));
        }
        let row = edit.row.clone();
        self.editing_secret = None;
        config::notify_success("Secret saved", &row);
        self.reload();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::ProcessOutput;
    use openidea_plugin::views::ViewEvent;

    use super::*;

    const JSON: &str = r#"{"apiVersion":"v1","data":{"user":"YWRtaW4=","blob":"/+8="},"kind":"Secret","metadata":{"name":"db","resourceVersion":"41"},"type":"Opaque"}"#;

    type Runs = std::rc::Rc<std::cell::RefCell<Vec<(Vec<String>, Option<String>)>>>;

    fn host(allow: bool, replace_stderr: &'static str) -> (MockHost, Runs) {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "storage"]);
        if allow {
            host.set_setting("allowRevealSecrets", "true");
        }
        let runs: Runs = Default::default();
        let seen = runs.clone();
        host.on_process(KUBECTL, move |args, stdin| {
            seen.borrow_mut().push((args.to_vec(), stdin.map(str::to_owned)));
            match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
                Some("get") => Ok(ProcessOutput { status: Some(0), stdout: JSON.into(), stderr: String::new() }),
                Some("replace") if !replace_stderr.is_empty() => {
                    Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: replace_stderr.into() })
                }
                _ => Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() }),
            }
        });
        (host, runs)
    }

    fn editing() -> KubeExplorer {
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };
        kube.edit_secret("db").unwrap();
        kube
    }

    fn type_into(kube: &mut KubeExplorer, key: &str, value: &str) {
        kube.editing_secret.as_mut().unwrap().set_field(key, value.into());
    }

    #[test]
    fn replace_data_keeps_everything_but_data() {
        let replaced = replace_data(JSON, &[("k".into(), b"v".to_vec())]).unwrap();
        assert_eq!(replaced, JSON.replace(r#"{"user":"YWRtaW4=","blob":"/+8="}"#, r#"{"k":"dg=="}"#));
        let bare = replace_data(r#"{"kind":"Secret"}"#, &[("k".into(), b"v".to_vec())]).unwrap();
        assert_eq!(bare, r#"{"data":{"k":"dg=="},"kind":"Secret"}"#);
        assert!(replace_data("[1]", &[]).is_err());
    }

    #[test]
    fn the_form_is_prefilled_and_binary_values_are_left_out() {
        let (_host, _) = host(true, "");
        let kube = editing();
        let Body::Form(fields) = kube.render_secret_edit(kube.editing_secret.as_ref().unwrap()).body else { panic!() };
        assert_eq!(fields[0].key, "secret-edit:user");
        assert_eq!(fields[0].value, FieldValue::Multiline("admin".into()));
        assert!(fields.iter().all(|f| f.key != "secret-edit:blob"));
    }

    #[test]
    fn save_replaces_the_object_with_its_resource_version_through_stdin() {
        let (_host, runs) = host(true, "");
        let mut kube = editing();
        type_into(&mut kube, "secret-edit:user", "root");
        type_into(&mut kube, "secret-new-key", "token");
        type_into(&mut kube, "secret-new-value", "t0p");
        kube.button("submit-secret-edit").unwrap();
        let (args, stdin) = runs.borrow().last().cloned().unwrap();
        assert_eq!(args, ["--namespace=shop", "replace", "-f", "-"]);
        let manifest = stdin.unwrap();
        assert!(manifest.contains(r#""resourceVersion":"41""#));
        for (key, value) in [("user", "root"), ("token", "t0p")] {
            assert!(manifest.contains(&format!("\"{key}\":\"{}\"", b64_encode(value.as_bytes()))));
        }
        assert!(manifest.contains(r#""blob":"/+8=""#));
        assert!(!args.iter().any(|a| a.contains("t0p") || a.contains("root")));
        assert!(kube.editing_secret.is_none());
    }

    #[test]
    fn removed_keys_are_dropped() {
        let (_host, runs) = host(true, "");
        let mut kube = editing();
        type_into(&mut kube, "secret-remove", "user blob");
        kube.button("submit-secret-edit").unwrap();
        let manifest = runs.borrow().last().unwrap().1.clone().unwrap();
        assert!(manifest.contains(r#""data":{}"#));
    }

    #[test]
    fn a_conflict_keeps_the_form_and_leaks_no_value() {
        let (_host, _) = host(true, "Operation cannot be fulfilled on secrets \"db\": the object has been modified; root");
        let mut kube = editing();
        type_into(&mut kube, "secret-edit:user", "root");
        let error = format!("{:?}", kube.button("submit-secret-edit").unwrap_err());
        assert!(error.contains("changed in the cluster") && !error.contains("root"));
        assert!(kube.editing_secret.is_some());
    }

    #[test]
    fn an_invalid_new_key_is_refused_before_kubectl_runs() {
        let (_host, runs) = host(true, "");
        let mut kube = editing();
        runs.borrow_mut().clear();
        type_into(&mut kube, "secret-new-key", "bad key!");
        type_into(&mut kube, "secret-new-value", "x");
        assert!(kube.button("submit-secret-edit").is_err());
        assert!(runs.borrow().is_empty());
    }

    #[test]
    fn editing_needs_the_reveal_setting_and_back_discards_the_form() {
        let (host, runs) = host(false, "");
        let mut kube = KubeExplorer::default();
        kube.edit_secret("db").unwrap();
        assert!(kube.editing_secret.is_none() && runs.borrow().is_empty());
        assert!(host.notifications()[0].body.contains("allowRevealSecrets"));

        host.set_setting("allowRevealSecrets", "true");
        let mut kube = editing();
        kube.button("back").unwrap();
        assert!(kube.editing_secret.is_none());
    }

    #[test]
    fn field_events_reach_the_form() {
        let (_host, _) = host(true, "");
        let mut kube = editing();
        kube.handle_tree(ViewEvent::Field("secret-edit:user".into(), "x".into())).unwrap();
        assert_eq!(kube.editing_secret.as_ref().unwrap().entries[0].1, "x");
    }
}
