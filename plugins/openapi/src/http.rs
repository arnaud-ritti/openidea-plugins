//! Requests for OpenIDEA's HTTP client (`.http` files).

use crate::spec::{Spec, operations};

fn base_url(spec: &Spec) -> String {
    let value = |key: &str| spec.keys.iter().find(|k| k.key == key).map(|k| k.value.as_str()).filter(|v| !v.is_empty());

    let base = if spec.version.starts_with("2.") {
        let scheme = value("schemes[0]").unwrap_or("http");
        let host = value("host").unwrap_or("localhost");
        format!("{scheme}://{host}{}", value("basePath").unwrap_or(""))
    } else {
        match value("servers[0].url") {
            Some(url) if url.starts_with('/') => format!("http://localhost{url}"),
            Some(url) => url.to_owned(),
            None => "http://localhost".to_owned(),
        }
    };

    base.trim_end_matches('/').to_owned()
}

fn variables(url: &str) -> String {
    url.replace('{', "{{").replace('}', "}}")
}

pub fn requests(text: &str, spec: &Spec) -> String {
    let base = base_url(spec);

    operations(text, &spec.keys)
        .iter()
        .map(|op| {
            let title = op.operation_id.clone().unwrap_or_else(|| format!("{} {}", op.method, op.path));
            format!("### {title}\n{} {}\n", op.method, variables(&format!("{base}{}", op.path)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{PETSTORE, parse};

    #[test]
    fn one_request_per_operation_with_variables() {
        assert_eq!(
            requests(PETSTORE, &parse(PETSTORE).unwrap()),
            "### listPets\nGET https://api.example.com/v1/pets\n\n### showPet\nGET https://api.example.com/v1/pets/{{petId}}\n\n### deletePet\nDELETE https://api.example.com/v1/pets/{{petId}}\n"
        );
    }

    #[test]
    fn swagger_2_base_url_from_scheme_host_and_base_path() {
        let text = "swagger: '2.0'\nschemes:\n  - https\nhost: api.example.com\nbasePath: /v2\npaths:\n  /pets:\n    post:\n      responses: {}\n";
        assert_eq!(requests(text, &parse(text).unwrap()), "### POST /pets\nPOST https://api.example.com/v2/pets\n");
    }

    #[test]
    fn defaults_to_localhost_and_keeps_server_variables() {
        let ops = "paths:\n  /a:\n    get:\n      responses: {}\n";
        let first_line = |head: &str| {
            let text = format!("openapi: 3.1.0\n{head}{ops}");
            requests(&text, &parse(&text).unwrap()).lines().nth(1).unwrap().to_owned()
        };
        assert_eq!(first_line("servers:\n  - url: '{scheme}://example.com/'\n"), "GET {{scheme}}://example.com/a");
        assert_eq!(first_line(""), "GET http://localhost/a");
        assert_eq!(first_line("servers:\n  - url: /api\n"), "GET http://localhost/api/a");
    }
}
