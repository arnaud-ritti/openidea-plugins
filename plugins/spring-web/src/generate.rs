use openidea_plugin::providers::CodeAction;
use openidea_plugin::text::offset;
use openidea_plugin::{Range, TextEdit};
use spring_text::java::{self, JavaFile, TypeDecl};
use spring_text::{parse_source, range};

const ANNOTATION_PACKAGE: &str = "org.springframework.web.bind.annotation";

struct Template {
    title: &'static str,
    annotation: &'static str,
    signature: &'static str,
    name: &'static str,
    mark: &'static str,
    kotlin_mark: &'static str,
}

const REQUEST_MAPPING: Template = Template {
    title: "Add @RequestMapping method",
    annotation: "RequestMapping",
    signature: "@RequestMapping(\"/path\")",
    name: "handle",
    mark: "public String handle() {",
    kotlin_mark: "fun handle(): String {",
};

const EXCEPTION_HANDLER: Template = Template {
    title: "Add @ExceptionHandler method",
    annotation: "ExceptionHandler",
    signature: "@ExceptionHandler(Exception.class)",
    name: "handleException",
    mark: "public String handleException(Exception exception) {",
    kotlin_mark: "fun handleException(exception: Exception): String {",
};

pub fn actions(doc_path: &str, text: &str, selection: Range) -> Vec<CodeAction> {
    let kotlin = doc_path.ends_with(".kt");
    if !kotlin && !doc_path.ends_with(".java") {
        return Vec::new();
    }

    let file = parse_source(doc_path, text);
    let Some(caret) = offset(text, selection.start) else {
        return Vec::new();
    };
    let Some(decl) = enclosing_type(&file, text, caret) else {
        return Vec::new();
    };

    if decl.members.iter().any(|m| m.start <= caret && caret < m.end) {
        return Vec::new();
    }

    let controllers = ["Controller", "RestController"];
    let advices = ["ControllerAdvice", "RestControllerAdvice"];
    let spring = controllers.iter().chain(&advices).all(|n| file.from_spring(n));
    let mut templates = Vec::new();
    if spring && decl.has(&controllers) {
        templates.push(&REQUEST_MAPPING);
    }
    if spring && (decl.has(&controllers) || decl.has(&advices)) {
        templates.push(&EXCEPTION_HANDLER);
    }

    templates
        .into_iter()
        .map(|template| CodeAction {
            title: template.title.into(),
            kind: Some("refactor".into()),
            preferred: false,
            edits: vec![(doc_path.to_owned(), edits(&file, decl, text, template, kotlin))],
            data: None,
        })
        .collect()
}

fn enclosing_type<'f>(file: &'f JavaFile, text: &str, caret: usize) -> Option<&'f TypeDecl> {
    file.types
        .iter()
        .filter(|t| {
            let Some(open) = text[t.at..t.end].find('{') else {
                return false;
            };
            t.at + open < caret && caret < t.end
        })
        .min_by_key(|t| t.end - t.start)
}

fn edits(file: &JavaFile, decl: &TypeDecl, text: &str, template: &Template, kotlin: bool) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    edits.extend(import_edit(file, text, template.annotation, kotlin));
    edits.push(method_edit(decl, text, template, kotlin));
    edits
}

fn import_edit(file: &JavaFile, text: &str, annotation: &str, kotlin: bool) -> Option<TextEdit> {
    let qualified = format!("{ANNOTATION_PACKAGE}.{annotation}");
    let wildcard = format!("{ANNOTATION_PACKAGE}.*");
    let imported = file.imports.iter().any(|i| !i.is_static && (i.path == qualified || i.path == wildcard));
    if imported {
        return None;
    }

    let newline = newline(text);
    let line = if kotlin { format!("import {qualified}") } else { format!("import {qualified};") };
    let end_of = |from: usize| match kotlin {
        true => Some(line_end(text, from)),
        false => text[from..].find(';').map(|semicolon| from + semicolon + 1),
    };
    let after = match file.imports.iter().filter(|i| !i.is_static).chain(file.imports.iter().filter(|i| i.is_static)).max_by_key(|i| i.end) {
        Some(last) => end_of(last.end).map(|at| (at, format!("{newline}{line}"))),
        None => package_end(text, kotlin).map(|at| (at, format!("{newline}{newline}{line}"))),
    };
    let (at, inserted) = after.unwrap_or((0, format!("{line}{newline}{newline}")));

    Some(TextEdit { range: range(text, at, at), text: inserted })
}

fn line_end(text: &str, from: usize) -> usize {
    let end = text[from..].find('\n').map_or(text.len(), |n| from + n);
    text[..end].trim_end_matches('\r').len()
}

fn package_end(text: &str, kotlin: bool) -> Option<usize> {
    let tokens = java::tokens(text);
    let package = tokens.iter().position(|t| t.s == "package")?;
    if kotlin {
        return Some(line_end(text, tokens[package].at));
    }

    tokens[package..].iter().find(|t| t.s == ";").map(|t| t.at + 1)
}

fn method_edit(decl: &TypeDecl, text: &str, template: &Template, kotlin: bool) -> TextEdit {
    let newline = newline(text);
    let indent = indent(decl, text);
    let body_indent = format!("{indent}{}", indent_unit(&indent));
    let name = free_name(decl, template.name);
    let mark = if kotlin { template.kotlin_mark } else { template.mark }.replace(template.name, &name);
    let signature = if kotlin { template.signature.replace(".class)", "::class)") } else { template.signature.to_owned() };
    let semicolon = if kotlin { "" } else { ";" };
    let method = format!("{indent}{signature}{newline}{indent}{mark}{newline}{body_indent}return \"\"{semicolon}{newline}{indent}}}{newline}");

    let close = decl.end - 1;
    let line_start = text[..close].rfind('\n').map_or(0, |n| n + 1);
    let alone = text[line_start..close].trim().is_empty();
    let at = if alone { line_start } else { close };
    let blank_before = text[..line_start].trim_end_matches([' ', '\t']).ends_with("\n\n")
        || text[..line_start].trim_end_matches([' ', '\t']).ends_with("\r\n\r\n");
    let lead = if decl.members.is_empty() || blank_before { "" } else { newline };
    let inserted = if alone {
        format!("{lead}{method}")
    } else {
        format!("{newline}{method}")
    };

    TextEdit { range: range(text, at, at), text: inserted }
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn indent(decl: &TypeDecl, text: &str) -> String {
    let Some(first) = decl.members.first() else {
        return if text.lines().any(|l| l.starts_with('\t')) { "\t".into() } else { "    ".into() };
    };

    let line_start = text[..first.start].rfind('\n').map_or(0, |n| n + 1);
    text[line_start..first.start].chars().take_while(|c| c.is_whitespace()).collect()
}

fn indent_unit(indent: &str) -> &'static str {
    if indent.contains('\t') { "\t" } else { "    " }
}

fn free_name(decl: &TypeDecl, base: &str) -> String {
    let taken = |name: &str| decl.members.iter().any(|m| m.name == name);
    if !taken(base) {
        return base.to_owned();
    }

    (2..).map(|n| format!("{base}{n}")).find(|name| !taken(name)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Position;

    const PATH: &str = "/p/UserController.java";

    const CONTROLLER: &str = "package com.acme;\n\nimport org.springframework.stereotype.Controller;\n\n@Controller\nclass UserController {\n    @GetMapping\n    String list() { return \"\"; }\n\n}\n";

    fn at(line: u32, column: u32) -> Range {
        Range::new(Position::new(line, column), Position::new(line, column))
    }

    fn titles(text: &str, selection: Range) -> Vec<String> {
        actions(PATH, text, selection).into_iter().map(|a| a.title).collect()
    }

    fn apply(text: &str, action: &CodeAction) -> String {
        let mut edits: Vec<&TextEdit> = action.edits[0].1.iter().collect();
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        let mut out = text.to_owned();
        for edit in edits {
            let from = offset(&out, edit.range.start).unwrap();
            let to = offset(&out, edit.range.end).unwrap();
            out.replace_range(from..to, &edit.text);
        }
        out
    }

    #[test]
    fn controller_body_offers_request_mapping_and_exception_handler() {
        assert_eq!(
            titles(CONTROLLER, at(8, 0)),
            vec!["Add @RequestMapping method", "Add @ExceptionHandler method"]
        );

        let first = &actions(PATH, CONTROLLER, at(8, 0))[0];
        assert_eq!(first.kind.as_deref(), Some("refactor"));
        assert!(!first.preferred);
    }

    #[test]
    fn controller_advice_offers_only_exception_handler() {
        let advice = CONTROLLER.replace("@Controller", "@ControllerAdvice").replace("stereotype.Controller", "web.bind.annotation.ControllerAdvice");

        assert_eq!(titles(&advice, at(8, 0)), vec!["Add @ExceptionHandler method"]);
    }

    #[test]
    fn selection_inside_a_method_offers_nothing() {
        assert!(titles(CONTROLLER, at(7, 10)).is_empty());
    }

    #[test]
    fn non_controller_class_offers_nothing() {
        let plain = CONTROLLER.replace("@Controller\n", "");

        assert!(titles(&plain, at(8, 0)).is_empty());
        assert!(titles(CONTROLLER, at(2, 3)).is_empty());
    }

    #[test]
    fn generated_method_is_inserted_before_the_closing_brace() {
        let action = &actions(PATH, CONTROLLER, at(8, 0))[0];
        let applied = apply(CONTROLLER, action);

        assert!(applied.contains("import org.springframework.web.bind.annotation.RequestMapping;"));
        assert!(applied.ends_with(
            "\n\n    @RequestMapping(\"/path\")\n    public String handle() {\n        return \"\";\n    }\n}\n"
        ));
    }

    #[test]
    fn blank_line_before_the_closing_brace_is_not_doubled() {
        let with_blank = CONTROLLER;
        let without_blank = CONTROLLER.replace("\n\n}\n", "\n}\n");
        let method = "    @RequestMapping(\"/path\")\n    public String handle() {\n        return \"\";\n    }\n";

        let edit = |text: &str| actions(PATH, text, at(8, 0))[0].edits[0].1[1].text.clone();

        assert_eq!(edit(with_blank), method);
        assert_eq!(edit(&without_blank), format!("\n{method}"));
    }

    #[test]
    fn package_in_a_header_comment_is_ignored() {
        let text = "/* licensed under the package terms; see LICENSE */\npackage com.acme;\n\n@Controller\nclass A {\n}\n";
        let applied = apply(text, &actions(PATH, text, at(4, 9))[0]);

        assert!(applied.contains(
            "package com.acme;\n\nimport org.springframework.web.bind.annotation.RequestMapping;\n\n@Controller"
        ));
    }

    #[test]
    fn imports_are_added_only_when_missing() {
        let imported = CONTROLLER.replace(
            "import org.springframework.stereotype.Controller;",
            "import org.springframework.stereotype.Controller;\nimport org.springframework.web.bind.annotation.RequestMapping;",
        );
        let wildcard = CONTROLLER.replace(
            "import org.springframework.stereotype.Controller;",
            "import org.springframework.stereotype.Controller;\nimport org.springframework.web.bind.annotation.*;",
        );

        assert_eq!(actions(PATH, &imported, at(9, 0))[0].edits[0].1.len(), 1);
        assert_eq!(actions(PATH, &wildcard, at(9, 0))[0].edits[0].1.len(), 1);
        assert_eq!(actions(PATH, CONTROLLER, at(8, 0))[0].edits[0].1.len(), 2);
    }

    #[test]
    fn imports_go_after_the_package_without_other_imports() {
        let bare = "package com.acme;\n\n@org.springframework.stereotype.Controller\nclass A {\n}\n";
        let bare = bare.replace("@org.springframework.stereotype.Controller", "@Controller");
        let applied = apply(&bare, &actions(PATH, &bare, at(3, 9))[0]);

        assert!(applied.starts_with(
            "package com.acme;\n\nimport org.springframework.web.bind.annotation.RequestMapping;\n\n@Controller"
        ));
    }

    #[test]
    fn generated_method_keeps_crlf_and_tabs() {
        let text = CONTROLLER.replace("    ", "\t").replace('\n', "\r\n");
        let applied = apply(&text, &actions(PATH, &text, at(8, 0))[0]);

        assert!(applied.contains("\t@RequestMapping(\"/path\")\r\n\tpublic String handle() {\r\n\t\treturn \"\";\r\n\t}\r\n}"));
        assert!(!applied.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn generated_method_name_avoids_collisions() {
        let text = CONTROLLER.replace("String list()", "String handle()");
        let applied = apply(&text, &actions(PATH, &text, at(8, 0))[0]);

        assert!(applied.contains("public String handle2() {"));
    }

    #[test]
    fn exception_handler_text() {
        let text = CONTROLLER.replace("String list()", "String handleException()");
        let applied = apply(&text, &actions(PATH, &text, at(8, 0))[1]);

        assert!(applied.contains(
            "@ExceptionHandler(Exception.class)\n    public String handleException2(Exception exception) {"
        ));
    }

    const KOTLIN: &str = "package com.acme\n\nimport org.springframework.stereotype.Controller\n\n@Controller\nclass UserController {\n    @GetMapping\n    fun list(): String = \"\"\n\n}\n";

    #[test]
    fn kotlin_controller_gets_kotlin_methods() {
        let path = "/p/UserController.kt";
        let all = actions(path, KOTLIN, at(8, 0));
        assert_eq!(all.len(), 2);
        let applied = apply(KOTLIN, &all[0]);
        assert!(applied.contains("import org.springframework.stereotype.Controller\nimport org.springframework.web.bind.annotation.RequestMapping\n"));
        assert!(applied.contains("    @RequestMapping(\"/path\")\n    fun handle(): String {\n        return \"\"\n    }\n}"));
        let handler = apply(KOTLIN, &all[1]);
        assert!(handler.contains("@ExceptionHandler(Exception::class)\n    fun handleException(exception: Exception): String {"));
    }

    #[test]
    fn kotlin_import_goes_after_the_package_line_without_imports() {
        let bare = "package com.acme\n\n@Controller\nclass A {\n}\n";
        let applied = apply(bare, &actions("/p/A.kt", bare, at(3, 9))[0]);
        assert!(applied.starts_with("package com.acme\n\nimport org.springframework.web.bind.annotation.RequestMapping\n\n@Controller"));
    }

    #[test]
    fn other_extensions_offer_nothing() {
        assert!(actions("/p/UserController.groovy", CONTROLLER, at(8, 0)).is_empty());
    }

    #[test]
    fn generate_is_quiet_for_library_or_dynamic_name() {
        let foreign = CONTROLLER.replace("org.springframework.stereotype.Controller", "com.acme.Controller");

        assert!(titles(&foreign, at(8, 0)).is_empty());
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let line = "import org.springframework.stereotype.Controller /* héllo 😀 */;";
        let text = CONTROLLER.replace("import org.springframework.stereotype.Controller;", line);
        let action = &actions(PATH, &text, at(8, 0))[0];

        let import = &action.edits[0].1[0];
        assert_eq!(import.range.start, Position::new(2, line.encode_utf16().count() as u32));
        assert!(apply(&text, action).contains("public String handle() {"));
    }
}
