//! A problem a text check found, as bytes of the source it ran on.

use crate::range;
use openidea_plugin::{Diagnostic, Severity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
}

impl Finding {
    pub fn new(start: usize, end: usize, code: &'static str, message: impl Into<String>) -> Self {
        Self { start, end, code, message: message.into() }
    }

    /// The warning for the editor, its range in UTF-16 columns of `text`.
    pub fn diagnostic(&self, text: &str) -> Diagnostic {
        Diagnostic {
            code: Some(self.code.into()),
            ..Diagnostic::new(range(text, self.start, self.end), Severity::Warning, &self.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_to_a_warning_with_its_code() {
        let text = "a\nbc";
        let d = Finding::new(2, 4, "some-code", "Message.").diagnostic(text);
        assert_eq!((d.range.start.line, d.range.start.character, d.range.end.character), (1, 0, 2));
        assert_eq!((d.code.as_deref(), d.severity), (Some("some-code"), Severity::Warning));
        assert_eq!(d.message, "Message.");
    }

    #[test]
    fn columns_count_utf16_units() {
        let text = "é😀x";
        let d = Finding::new(text.find('x').unwrap(), text.len(), "c", "m").diagnostic(text);
        assert_eq!(d.range.start.character, 3);
        assert_eq!(d.range.end.character, 4);
    }
}
