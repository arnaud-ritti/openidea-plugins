//! An XML element scanner: start tags with their attributes, nesting and
//! extent. Comments, CDATA, processing instructions and the doctype are
//! skipped; entities are left as written. Offsets are UTF-8 bytes.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    /// As written (`batch:next`).
    pub name: String,
    pub at: usize,
    pub value: String,
    /// The bytes between the quotes.
    pub value_at: usize,
    pub value_end: usize,
}

impl Attr {
    /// The name without its namespace prefix.
    pub fn local(&self) -> &str {
        local(&self.name)
    }

    /// Whether the byte offset `at` is inside the value's quotes (or right after its last character).
    pub fn holds(&self, at: usize) -> bool {
        self.value_at <= at && at <= self.value_end
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// As written (`int:channel`).
    pub name: String,
    /// Byte offset of the name.
    pub at: usize,
    /// The `<` of the start tag.
    pub start: usize,
    /// After the start tag's `>`.
    pub tag_end: usize,
    /// After the end tag (or the start tag when it closes itself, or the text if unclosed).
    pub end: usize,
    pub attrs: Vec<Attr>,
    /// Index of the parent element.
    pub parent: Option<usize>,
}

impl Element {
    pub fn local(&self) -> &str {
        local(&self.name)
    }

    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name == name || a.local() == name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.attr(name).map(|a| a.value.as_str())
    }
}

pub fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Every element of `text`, in document order.
pub fn elements(text: &str) -> Vec<Element> {
    let b = text.as_bytes();
    let find = |from: usize, pat: &str| text[from.min(text.len())..].find(pat).map_or(text.len(), |p| from + p + pat.len());
    let name_char = |c: u8| !c.is_ascii_whitespace() && !matches!(c, b'>' | b'/' | b'=' | b'<' | b'"' | b'\'');
    let mut out: Vec<Element> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut i = 0;
    while let Some(lt) = text[i..].find('<').map(|p| i + p) {
        let rest = &text[lt..];
        if rest.starts_with("<!--") {
            i = find(lt + 4, "-->");
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i = find(lt + 9, "]]>");
            continue;
        }
        if rest.starts_with("<?") {
            i = find(lt + 2, "?>");
            continue;
        }
        if rest.starts_with("<!") {
            i = find(lt + 2, ">");
            continue;
        }
        if rest.starts_with("</") {
            let mut j = lt + 2;
            while j < b.len() && name_char(b[j]) {
                j += 1;
            }
            let name = &text[lt + 2..j];
            let end = find(j, ">");
            // Close up to the matching element; a stray end tag closes nothing.
            if let Some(pos) = open.iter().rposition(|&e| out[e].name == name) {
                for e in open.drain(pos..) {
                    out[e].end = end;
                }
            }
            i = end;
            continue;
        }
        let mut j = lt + 1;
        while j < b.len() && name_char(b[j]) {
            j += 1;
        }
        if j == lt + 1 {
            i = lt + 1;
            continue;
        }
        let mut el = Element {
            name: text[lt + 1..j].to_owned(),
            at: lt + 1,
            start: lt,
            tag_end: text.len(),
            end: text.len(),
            attrs: Vec::new(),
            parent: open.last().copied(),
        };
        let mut self_closing = false;
        while j < b.len() {
            let c = b[j];
            if c.is_ascii_whitespace() {
                j += 1;
            } else if c == b'>' {
                j += 1;
                break;
            } else if c == b'/' && b.get(j + 1) == Some(&b'>') {
                self_closing = true;
                j += 2;
                break;
            } else if c == b'<' {
                break; // an unfinished tag
            } else if name_char(c) {
                let at = j;
                while j < b.len() && name_char(b[j]) {
                    j += 1;
                }
                let name = text[at..j].to_owned();
                let mut k = j;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                if b.get(k) != Some(&b'=') {
                    el.attrs.push(Attr { name, at, value: String::new(), value_at: j, value_end: j });
                    continue;
                }
                k += 1;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                match b.get(k) {
                    Some(&q) if q == b'"' || q == b'\'' => {
                        let value_at = k + 1;
                        let value_end = text[value_at..].find(q as char).map_or(text.len(), |p| value_at + p);
                        el.attrs.push(Attr {
                            name,
                            at,
                            value: text[value_at..value_end].to_owned(),
                            value_at,
                            value_end,
                        });
                        j = (value_end + 1).min(text.len());
                    }
                    _ => j = k,
                }
            } else {
                j += 1;
            }
        }
        el.tag_end = j;
        if self_closing {
            el.end = j;
        }
        out.push(el);
        if !self_closing {
            open.push(out.len() - 1);
        }
        i = j.max(lt + 1);
    }
    out
}

/// The element whose start tag holds the byte offset `at`.
pub fn element_at(elements: &[Element], at: usize) -> Option<usize> {
    elements.iter().rposition(|e| e.start < at && at < e.tag_end)
}

/// The root element's local name (`beans`, `flow`, `schema`…).
pub fn root(elements: &[Element]) -> Option<&str> {
    elements.first().map(|e| e.local())
}

/// The namespace URI of `elements[i]`: its prefix (or the default namespace)
/// looked up in the `xmlns` declarations of the element and its ancestors.
pub fn namespace(elements: &[Element], i: usize) -> Option<&str> {
    let decl = elements[i].name.split_once(':').map_or_else(|| "xmlns".to_owned(), |(p, _)| format!("xmlns:{p}"));
    let mut at = Some(i);
    while let Some(e) = at {
        if let Some(a) = elements[e].attrs.iter().find(|a| a.name == decl) {
            return Some(&a.value);
        }
        at = elements[e].parent;
    }
    None
}

/// Bean names declared in a Spring XML file: `<bean id>` and its `name`
/// aliases, `<alias alias>`, and the `id` of any other top-level element
/// (custom namespaces such as `<int:channel id>` register beans too).
pub fn bean_ids(elements: &[Element]) -> Vec<&Attr> {
    let mut out = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        let top = e.parent.is_none_or(|p| elements[p].local() == "beans") || e.local() == "bean";
        if !top || i == 0 && e.local() == "beans" {
            continue;
        }
        match e.local() {
            "alias" => out.extend(e.attr("alias")),
            _ => {
                out.extend(e.attr("id"));
                if e.local() == "bean" {
                    out.extend(e.attr("name"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?>
<!DOCTYPE beans>
<beans xmlns="http://www.springframework.org/schema/beans" xmlns:int="x">
  <!-- <bean id="commented"/> -->
  <bean id="a" name="b" class="com.acme.A">
    <property name="next" ref="c"/>
    <constructor-arg><ref bean='d'/></constructor-arg>
    <![CDATA[ <bean id="cdata"/> ]]>
  </bean>
  <int:channel id="orders"/>
  <alias name="a" alias="z"/>
  <broken attr=
</beans>"#;

    #[test]
    fn scans_elements() {
        let els = elements(XML);
        let names: Vec<_> = els.iter().map(|e| (e.name.as_str(), e.parent)).collect();
        assert_eq!(names, [
            ("beans", None),
            ("bean", Some(0)),
            ("property", Some(1)),
            ("constructor-arg", Some(1)),
            ("ref", Some(3)),
            ("int:channel", Some(0)),
            ("alias", Some(0)),
            ("broken", Some(0)),
        ]);
        let r = els[2].attr("ref").unwrap();
        assert_eq!(&XML[r.value_at..r.value_end], "c");
        assert_eq!(els[4].value("bean"), Some("d"));
        assert_eq!(els[5].local(), "channel");
        assert!(XML[els[1].start..els[1].end].ends_with("</bean>"));
        let ids: Vec<_> = bean_ids(&els).into_iter().map(|a| a.value.as_str()).collect();
        assert_eq!(ids, ["a", "b", "orders", "z"]);
        assert_eq!(root(&els), Some("beans"));
        assert_eq!(namespace(&els, 2), Some("http://www.springframework.org/schema/beans"));
        assert_eq!(namespace(&els, 5), Some("x"));
    }

    #[test]
    fn finds_the_tag_under_the_caret() {
        let els = elements(XML);
        let at = XML.find("ref=\"c").unwrap() + 5;
        assert_eq!(element_at(&els, at), Some(2));
    }

    #[test]
    fn survives_broken_input() {
        for src in ["<", "<a", "<a b='", "</", "<!--", "<a></b></a>", "<é attr=\"é\"/>"] {
            elements(src);
        }
        for end in 0..=XML.len() {
            elements(&XML[..end]);
        }
    }
}
