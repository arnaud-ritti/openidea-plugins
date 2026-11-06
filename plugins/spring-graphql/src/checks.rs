//! Text checks over one controller. A check stays quiet when an annotation
//! comes from a library other than Spring or the return type is not clearly invalid.

use spring_text::finding::Finding;
use spring_text::java::{JavaFile, TypeRef};

const INVALID_RETURNS: [&str; 24] = [
    "void", "Unit", "boolean", "byte", "short", "int", "long", "float", "double", "char", "Boolean", "Byte", "Short",
    "Integer", "Long", "Float", "Double", "Character", "String", "Object", "Set", "Collection", "Iterable", "Optional",
];

fn invalid_return(ty: &TypeRef) -> bool {
    if ty.array {
        return true;
    }

    if ty.name == "Mono" {
        return ty.args.first().is_some_and(|argument| argument.name != "Map");
    }

    INVALID_RETURNS.contains(&ty.name.as_str())
}

/// A `@BatchMapping` method returns a `Map` of source to value, a `List` aligned with the sources, a `Flux`, or a `Mono<Map>`; the clearly invalid shapes are reported, custom wrappers are not.
pub fn batch_mapping_return(file: &JavaFile) -> Vec<Finding> {
    if !file.from_spring("BatchMapping") || !file.from_spring("Controller") || !file.from_spring("RestController") {
        return Vec::new();
    }

    file.types
        .iter()
        .filter(|ty| ty.has(&["Controller", "RestController"]))
        .flat_map(|ty| ty.methods())
        .filter(|method| method.annotation("BatchMapping").is_some())
        .filter_map(|method| method.ty.as_ref())
        .filter(|ty| invalid_return(ty))
        .map(|ty| {
            Finding::new(
                ty.at,
                ty.at + ty.name.len(),
                "batch-mapping-return-type",
                format!("A @BatchMapping method returns a Map of source to value, a List of values in source order, a Flux or a Mono<Map>, not `{}`.", ty.text),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::java::parse;
    use spring_text::kotlin;

    fn controller(ret: &str) -> String {
        format!("import org.springframework.stereotype.Controller;\n@Controller\nclass C {{\n    @BatchMapping\n    {ret} titles(List<Book> books) {{ return null; }}\n}}\n")
    }

    fn flagged(text: &str) -> Vec<String> {
        batch_mapping_return(&parse(text)).iter().map(|f| text[f.start..f.end].to_owned()).collect()
    }

    #[test]
    fn void_batch_mapping_warns() {
        assert_eq!(flagged(&controller("void")), ["void"]);
    }

    #[test]
    fn string_batch_mapping_warns() {
        assert_eq!(flagged(&controller("String")), ["String"]);
        assert_eq!(flagged(&controller("int")), ["int"]);
        assert_eq!(flagged(&controller("Set<String>")), ["Set"]);
        assert_eq!(flagged(&controller("String[]")), ["String"]);
    }

    #[test]
    fn mono_of_list_batch_mapping_warns() {
        assert_eq!(flagged(&controller("Mono<List<String>>")), ["Mono"]);
    }

    #[test]
    fn valid_batch_mapping_returns_are_quiet() {
        for ret in ["Map<Book, String>", "List<String>", "Flux<String>", "Mono<Map<Book, String>>"] {
            assert!(flagged(&controller(ret)).is_empty(), "{ret}");
        }
    }

    #[test]
    fn batch_mapping_return_is_quiet_for_library_or_dynamic_name() {
        assert!(flagged(&controller("Callable<String>")).is_empty());
        assert!(flagged(&controller("Mono")).is_empty());
        let foreign = format!("import com.acme.BatchMapping;\n{}", controller("void"));
        assert!(flagged(&foreign).is_empty());
        let plain = controller("void").replace("@Controller", "@Component");
        assert!(flagged(&plain).is_empty());
    }

    #[test]
    fn kotlin_unit_batch_mapping_warns() {
        let text = "@Controller\nclass C {\n    @BatchMapping\n    fun titles(books: List<Book>) {}\n\n    @BatchMapping\n    fun ok(books: List<Book>): Map<Book, String> = emptyMap()\n}\n";
        let found: Vec<_> = batch_mapping_return(&kotlin::parse(text)).iter().map(|f| f.code).collect();
        assert_eq!(found, ["batch-mapping-return-type"]);
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        assert_eq!(flagged(&controller("void")).len(), 1);
        assert!(flagged(&controller("List<String>")).is_empty());
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let text = controller("String").replace("class C", "class Cé😀");
        let text = text.replace("    String titles", "    /* é😀 */ String titles");
        let d = batch_mapping_return(&parse(&text))[0].diagnostic(&text);
        assert_eq!((d.range.start.line, d.range.start.character), (4, 14));
        assert_eq!(d.range.end.character - d.range.start.character, 6);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let text = controller("void").replace('\n', "\r\n");
        let lines: Vec<_> = batch_mapping_return(&parse(&text)).iter().map(|f| f.diagnostic(&text).range.start.line).collect();
        assert_eq!(lines, [4]);
    }
}
