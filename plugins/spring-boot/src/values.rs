//! Whether a configuration value converts to the type its key's metadata names.
//! Only types whose conversion is simple and fixed are checked.

fn integer(value: &str, range: (i128, i128)) -> Option<String> {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    let valid = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    let parsed = valid.then(|| value.parse::<i128>().ok()).flatten();
    match parsed {
        Some(n) if (range.0..=range.1).contains(&n) => None,
        _ => Some(format!("`{value}` is not a valid number for this property.")),
    }
}

fn boolean(value: &str) -> Option<String> {
    let known = ["true", "false", "on", "off", "yes", "no", "1", "0"];
    (!known.contains(&value.to_ascii_lowercase().as_str())).then(|| format!("`{value}` is not a boolean (true or false)."))
}

fn suffixed(value: &str, units: &[&str]) -> bool {
    let rest = value.strip_prefix(['+', '-']).unwrap_or(value);
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    let unit = rest[digits..].to_ascii_lowercase();
    unit.is_empty() || units.contains(&unit.as_str())
}

fn duration(value: &str) -> Option<String> {
    let iso = value.trim_start_matches(['+', '-']).to_ascii_uppercase().starts_with('P');
    if iso || suffixed(value, &["ns", "us", "ms", "s", "m", "h", "d"]) {
        return None;
    }
    Some(format!("`{value}` is not a duration; use a number with ns, us, ms, s, m, h or d (`10s`) or ISO-8601 (`PT10S`)."))
}

fn data_size(value: &str) -> Option<String> {
    if suffixed(value, &["b", "kb", "mb", "gb", "tb"]) {
        return None;
    }
    Some(format!("`{value}` is not a data size; use a number with B, KB, MB, GB or TB (`10MB`)."))
}

/// The problem with `value` for a key of metadata type `ty`, or `None` when it converts or the type is not checked.
pub fn problem(ty: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains("${") || value.contains("#{") {
        return None;
    }
    match ty {
        "Integer" | "int" => integer(value, (i32::MIN.into(), i32::MAX.into())),
        "Long" | "long" => integer(value, (i64::MIN.into(), i64::MAX.into())),
        "Short" | "short" => integer(value, (i16::MIN.into(), i16::MAX.into())),
        "Boolean" | "boolean" => boolean(value),
        "Double" | "double" => value.parse::<f64>().is_err().then(|| format!("`{value}` is not a decimal number.")),
        "Duration" => duration(value),
        "DataSize" => data_size(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_or_reports() {
        assert!(problem("Integer", "8080").is_none());
        assert!(problem("Integer", "80x").is_some());
        assert!(problem("Integer", "99999999999").is_some());
        assert!(problem("Short", "70000").is_some());
        assert!(problem("Boolean", "TRUE").is_none());
        assert!(problem("Boolean", "maybe").is_some());
        assert!(problem("Duration", "10s").is_none());
        assert!(problem("Duration", "PT10S").is_none());
        assert!(problem("Duration", "10 seconds").is_some());
        assert!(problem("DataSize", "1MB").is_none());
        assert!(problem("DataSize", "1XB").is_some());
        assert!(problem("Double", "1.5").is_none());
        assert!(problem("Integer", "${PORT:80}").is_none());
        assert!(problem("Charset", "nonsense").is_none());
        assert!(problem("List<String>", "a,b").is_none());
    }
}
