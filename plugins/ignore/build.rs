// Embeds every github/gitignore template under `templates/` as
// `TEMPLATES: &[(name, content)]`, `name` without `.gitignore`, sorted.
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "gitignore") {
            out.push(path);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut code = String::from("pub static TEMPLATES: &[(&str, &str)] = &[\n");
    for file in files {
        let name = file.strip_prefix(&root).unwrap().with_extension("");
        let name = name.to_str().unwrap().replace('\\', "/");
        code += &format!("    ({name:?}, include_str!({:?})),\n", file.display().to_string());
    }
    code += "];\n";
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("templates.rs"), code).unwrap();
}
