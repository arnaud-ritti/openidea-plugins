//! The OpenAPI document model: version detection and the operations under `paths`.

pub use spring_text::config::Key;

pub const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

pub struct Spec {
    pub version: String,
    pub keys: Vec<Key>,
}

pub struct Operation {
    pub method: String,
    pub path: String,
    pub key: String,
    pub at: usize,
    pub end: usize,
    pub operation_id: Option<String>,
}

pub fn parse(text: &str) -> Option<Spec> {
    let keys = spring_text::config::yaml(text);
    let version = keys
        .iter()
        .find(|k| k.key == "openapi" || k.key == "swagger")
        .map(|k| k.value.clone())
        .filter(|v| !v.is_empty())?;

    Some(Spec { version, keys })
}

pub fn operations(text: &str, keys: &[Key]) -> Vec<Operation> {
    let paths: Vec<(&Key, &str)> = keys
        .iter()
        .map(|k| (k, &text[k.at..k.end]))
        .filter(|(k, segment)| segment.starts_with('/') && k.key == format!("paths.{segment}"))
        .collect();

    keys.iter()
        .filter_map(|k| {
            let method = &text[k.at..k.end];
            if !METHODS.contains(&method) {
                return None;
            }

            let (_, path) = paths.iter().find(|(p, _)| k.key == format!("{}.{method}", p.key))?;
            let operation_id_key = format!("{}.operationId", k.key);

            Some(Operation {
                method: method.to_uppercase(),
                path: path.to_string(),
                key: k.key.clone(),
                at: k.at,
                end: k.end,
                operation_id: keys.iter().find(|v| v.key == operation_id_key).map(|v| v.value.clone()),
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) const PETSTORE: &str = "openapi: 3.0.3
info:
  title: Pets
  version: 1.0.0
servers:
  - url: https://api.example.com/v1
paths:
  /pets:
    get:
      operationId: listPets
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: array
                items: { $ref: '#/components/schemas/Pet' }
  /pets/{petId}:
    parameters:
      - $ref: '#/components/parameters/PetId'
    get:
      operationId: showPet
      responses:
        '200':
          $ref: '#/components/responses/Pet'
    delete:
      operationId: deletePet
      parameters:
        - name: force
          in: query
      responses:
        '204':
          description: gone
components:
  schemas:
    Pet:
      type: object
      properties:
        id: { type: integer }
    com.acme.Error:
      type: object
  parameters:
    PetId:
      name: petId
      in: path
      required: true
      schema: { type: string }
  responses:
    Pet:
      description: a pet
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/Pet'
";

#[test]
fn detects_openapi_and_swagger_documents() {
    assert_eq!(parse(PETSTORE).unwrap().version, "3.0.3");
    assert_eq!(parse("swagger: \"2.0\"\ninfo: {}\n").unwrap().version, "2.0");
    assert!(parse("apiVersion: v1\nkind: Pod\n").is_none());
    assert!(parse("info:\n  openapi: 3.0.0\n").is_none());
    assert!(parse("openapi:\n").is_none());
}

#[test]
fn lists_operations_under_paths() {
    let spec = parse(PETSTORE).unwrap();
    let ops = operations(PETSTORE, &spec.keys);
    let summary: Vec<_> = ops.iter().map(|o| (o.method.as_str(), o.path.as_str(), o.operation_id.as_deref())).collect();
    assert_eq!(summary, [
        ("GET", "/pets", Some("listPets")),
        ("GET", "/pets/{petId}", Some("showPet")),
        ("DELETE", "/pets/{petId}", Some("deletePet")),
    ]);
    assert_eq!(&PETSTORE[ops[1].at..ops[1].end], "get");
    assert_eq!(ops[1].key, "paths./pets/{petId}.get");
}

#[test]
fn a_path_with_dots_keeps_its_method() {
    let text = "openapi: 3.1.0\npaths:\n  /v1.2/pets.json:\n    parameters: []\n    post:\n      responses: {}\n";
    let ops = operations(text, &parse(text).unwrap().keys);
    assert_eq!(ops.len(), 1);
    assert_eq!((ops[0].method.as_str(), ops[0].path.as_str(), ops[0].operation_id.as_deref()), ("POST", "/v1.2/pets.json", None));
}
