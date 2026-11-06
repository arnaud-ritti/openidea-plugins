//! Columns of `database/migrations`: `$table->string('name')` inside
//! `Schema::create('users', …)` and `Schema::table('users', …)`.

use crate::index::separated;
use crate::php;

pub struct Column {
    pub table: String,
    pub name: String,
    /// The Blueprint method that defines it.
    pub kind: String,
    pub offset: usize,
}

/// Blueprint methods whose first argument names the column.
const NAMED: &[&str] = &[
    "string", "char", "text", "tinyText", "mediumText", "longText", "integer", "tinyInteger", "smallInteger", "mediumInteger",
    "bigInteger", "unsignedInteger", "unsignedTinyInteger", "unsignedSmallInteger", "unsignedMediumInteger", "unsignedBigInteger",
    "increments", "bigIncrements", "tinyIncrements", "smallIncrements", "mediumIncrements", "foreignId", "foreignUuid", "foreignUlid",
    "float", "double", "decimal", "unsignedDecimal", "boolean", "enum", "set", "json", "jsonb", "date", "dateTime", "dateTimeTz",
    "time", "timeTz", "timestamp", "timestampTz", "year", "binary", "uuid", "ulid", "ipAddress", "macAddress", "geometry", "point",
    "vector",
];
const MORPHS: &[&str] = &["morphs", "nullableMorphs", "uuidMorphs", "nullableUuidMorphs", "ulidMorphs", "nullableUlidMorphs"];

/// Columns of one migration, from its text with comments blanked.
pub fn parse(text: &str) -> Vec<Column> {
    let mut columns = Vec::new();

    for needle in ["Schema::create(", "Schema::table("] {
        for (at, _) in text.match_indices(needle) {
            let open = at + needle.len() - 1;
            let Some(close) = php::closing(text, open) else { continue };
            let Some((table, _)) = php::arguments(text, open).into_iter().next().and_then(|r| php::literal(text, r)) else { continue };
            let Some(brace) = text[open..close].find('{').map(|i| open + i) else { continue };
            let Some(end) = php::closing(text, brace) else { continue };

            body_columns(text, table, brace..end, &mut columns);
        }
    }

    columns.sort_by_key(|c| c.offset);
    columns
}

fn body_columns(text: &str, table: &str, body: std::ops::Range<usize>, columns: &mut Vec<Column>) {
    for (relative, _) in text[body.clone()].match_indices("->") {
        let arrow = body.start + relative;
        let name_start = arrow + 2;
        let name_length = text[name_start..body.end].bytes().position(|b| !php::is_identifier(b)).unwrap_or(0);
        let method = &text[name_start..name_start + name_length];
        let after = text[name_start + name_length..body.end].trim_start();
        if method.is_empty() || !after.starts_with('(') {
            continue;
        }

        let open = body.end - after.len();
        let arguments = php::arguments(text, open);
        let first = arguments.first().and_then(|r| php::literal(text, r.clone()));
        let mut add = |name: String, offset: usize| {
            columns.push(Column { table: table.to_owned(), name, kind: method.to_owned(), offset });
        };

        match method {
            "id" => add(first.map_or("id".into(), |(n, _)| n.to_owned()), first.map_or(name_start, |(_, at)| at)),
            "softDeletes" | "softDeletesTz" => add(first.map_or("deleted_at".into(), |(n, _)| n.to_owned()), first.map_or(name_start, |(_, at)| at)),
            "timestamps" | "timestampsTz" | "nullableTimestamps" => {
                add("created_at".into(), name_start);
                add("updated_at".into(), name_start);
            }
            "rememberToken" => add("remember_token".into(), name_start),
            "foreignIdFor" => {
                let class = arguments.first().and_then(|r| php::class_constant(text, r.clone()));
                if let Some(class) = class {
                    let short = class.rsplit('\\').next().unwrap_or(class);
                    add(format!("{}_id", separated(short, '_')), name_start);
                }
            }
            _ if MORPHS.contains(&method) => {
                if let Some((name, at)) = first {
                    add(format!("{name}_id"), at);
                    add(format!("{name}_type"), at);
                }
            }
            _ if NAMED.contains(&method) => {
                if let Some((name, at)) = first {
                    add(name.to_owned(), at);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_columns_of_created_and_altered_tables() {
        let text = "<?php\nreturn new class extends Migration {\n    public function up(): void\n    {\n        Schema::create('posts', function (Blueprint $table) {\n            $table->id();\n            $table->string('title')->nullable()->index();\n            $table->foreignId('user_id')->constrained();\n            $table->foreignIdFor(Category::class);\n            $table->morphs('taggable');\n            $table->softDeletes();\n            $table->timestamps();\n        });\n\n        Schema::table('users', function (Blueprint $table) {\n            $table->boolean('is_admin')->default(false)->after('email');\n            $table->rememberToken();\n        });\n    }\n};\n";
        let columns = parse(text);
        let found: Vec<_> = columns.iter().map(|c| (c.table.as_str(), c.name.as_str(), c.kind.as_str())).collect();
        assert_eq!(
            found,
            [
                ("posts", "id", "id"),
                ("posts", "title", "string"),
                ("posts", "user_id", "foreignId"),
                ("posts", "category_id", "foreignIdFor"),
                ("posts", "taggable_id", "morphs"),
                ("posts", "taggable_type", "morphs"),
                ("posts", "deleted_at", "softDeletes"),
                ("posts", "created_at", "timestamps"),
                ("posts", "updated_at", "timestamps"),
                ("users", "is_admin", "boolean"),
                ("users", "remember_token", "rememberToken"),
            ]
        );
        assert_eq!(&text[columns[1].offset..columns[1].offset + 5], "title");
    }

    #[test]
    fn survives_half_typed_migrations() {
        let text = "Schema::create('a', function (Blueprint $table) { $table->string('b')->foreignId(";
        for end in 0..=text.len() {
            parse(&text[..end]);
        }
    }
}
