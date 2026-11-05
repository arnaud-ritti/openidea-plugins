//! Spring's cron expressions (`CronExpression`, `@Scheduled(cron = …)`): six
//! fields, second to day-of-week, or a macro; `-` disables a schedule.

const FIELDS: [(&str, u32, u32); 6] = [
    ("second", 0, 59),
    ("minute", 0, 59),
    ("hour", 0, 23),
    ("day-of-month", 1, 31),
    ("month", 1, 12),
    ("day-of-week", 0, 7),
];
const MACROS: [&str; 7] = ["@yearly", "@annually", "@monthly", "@weekly", "@daily", "@midnight", "@hourly"];
const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const DAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const DOM: usize = 3;
const MONTH: usize = 4;
const DOW: usize = 5;

/// Why `expr` is not a valid Spring cron expression; `None` when it is.
pub fn check(expr: &str) -> Option<String> {
    let expr = expr.trim();
    if expr == "-" || MACROS.iter().any(|m| m.eq_ignore_ascii_case(expr)) {
        return None;
    }
    if expr.starts_with('@') {
        return Some(format!("Unknown cron macro `{expr}`. Spring accepts {}.", MACROS.join(", ")));
    }
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 6 {
        return Some(format!(
            "Spring cron expressions have 6 fields (second minute hour day-of-month month day-of-week), but this one has {}.",
            fields.len()
        ));
    }
    fields.iter().enumerate().find_map(|(i, f)| field(i, f).err().map(|e| format!("Cron {} `{f}`: {e}.", FIELDS[i].0)))
}

/// Month and day names as numbers, the way Spring reads them (`MON` → 1, `SUN` → 7).
fn numbered(value: &str, names: &[&str]) -> String {
    let mut value = value.to_ascii_uppercase();
    for (n, name) in names.iter().enumerate() {
        value = value.replace(name, &(n + 1).to_string());
    }
    value
}

fn field(i: usize, value: &str) -> Result<(), String> {
    let value = match i {
        MONTH => numbered(value, &MONTHS),
        DOW => numbered(value, &DAYS),
        _ => value.to_owned(),
    };
    if i == DOM && value.contains(['L', 'W']) {
        return last_or_weekday(&value);
    }
    if i == DOW && value.contains(['L', '#']) {
        return nth_day_of_week(&value);
    }
    value.split(',').try_for_each(|item| range(i, item))
}

/// One item of a list: `*`, `?`, `n`, `a-b`, each optionally `/step`.
fn range(i: usize, item: &str) -> Result<(), String> {
    let (span, step) = match item.split_once('/') {
        Some((span, step)) => (span, Some(step)),
        None => (item, None),
    };
    if let Some(step) = step {
        match step.parse::<u32>() {
            Ok(0) => return Err("the step after `/` must be 1 or more".into()),
            Ok(_) => {}
            Err(_) => return Err(format!("step `{step}` is not a number")),
        }
    }
    match span {
        "*" => Ok(()),
        "?" if i == DOM || i == DOW => Ok(()),
        "?" => Err("`?` is only allowed in the day-of-month and day-of-week".into()),
        _ => match span.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (number(i, a)?, number(i, b)?);
                // Sunday is 0 as the start of a range (`SUN-SAT`).
                let a = if i == DOW && a == 7 { 0 } else { a };
                if a > b { Err(format!("range `{span}` ends before it starts")) } else { Ok(()) }
            }
            None => number(i, span).map(|_| ()),
        },
    }
}

fn number(i: usize, s: &str) -> Result<u32, String> {
    let (_, lo, hi) = FIELDS[i];
    let n: u32 = s.parse().map_err(|_| match (i, s) {
        (_, "") => "a value is missing".to_owned(),
        (MONTH, _) => format!("`{s}` is not a number or a month name (JAN-DEC)"),
        (DOW, _) => format!("`{s}` is not a number or a day name (MON-SUN)"),
        _ => format!("`{s}` is not a number"),
    })?;
    if n < lo || n > hi { Err(format!("{n} is outside {lo}-{hi}")) } else { Ok(n) }
}

/// `L`, `L-n`, `LW` or `nW` in the day-of-month.
fn last_or_weekday(value: &str) -> Result<(), String> {
    if value == "L" || value == "LW" {
        return Ok(());
    }
    if let Some(offset) = value.strip_prefix("L-") {
        return match offset.parse::<u32>() {
            Ok(0..=30) => Ok(()),
            _ => Err(format!("`L-{offset}`: the offset from the last day must be 0-30")),
        };
    }
    if let Some(day) = value.strip_suffix('W')
        && !day.contains(['L', 'W', ','])
    {
        return number(DOM, day).map(|_| ());
    }
    Err("use `L`, `L-n`, `LW` or `nW` (nearest weekday to day n) on its own".into())
}

/// `nL` (last day n of the month) or `n#k` (k-th day n) in the day-of-week.
fn nth_day_of_week(value: &str) -> Result<(), String> {
    if let Some(day) = value.strip_suffix('L')
        && !day.contains(['L', '#', ','])
    {
        if day.is_empty() {
            return Err("`L` needs a day before it (`5L`, `FRIL`: the last Friday)".into());
        }
        return number(DOW, day).map(|_| ());
    }
    if let Some((day, nth)) = value.split_once('#')
        && !day.contains(['L', ','])
    {
        number(DOW, day)?;
        return match nth.parse::<u32>() {
            Ok(1..=5) => Ok(()),
            _ => Err(format!("`#{nth}`: the week of the month must be 1-5")),
        };
    }
    Err("use `nL` or `n#k` (the k-th day n of the month) on its own".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_expressions() {
        for expr in [
            "0 0 * * * *",
            "*/10 * * * * *",
            "0 0 8-10 * * *",
            "0 0 6,19 * * *",
            "0 0/30 8-10 * * *",
            "0 0 9-17 * * MON-FRI",
            "0 0 0 25 12 ?",
            "0 0 0 ? * 1",
            "0 0 0 L * *",
            "0 0 0 L-3 * *",
            "0 0 0 1W * *",
            "0 0 0 LW * *",
            "0 0 0 ? * 5L",
            "0 0 0 ? * fril",
            "0 0 0 ? * 5#2",
            "0 0 0 ? * MON#1",
            "0 0 0 * JAN-MAR SUN",
            "0 0 0 * jan,jul *",
            "0 0 0 * * 0",
            "0 0 0 * * 7",
            "0 0 0 * * SUN-SAT",
            "0 0 0 * * 0-7",
            "0 0 0 1-31/2 * *",
            "5/15 * * * * *",
            "  0 0 * * * *  ",
            "0  0 *  * * *",
            "@daily",
            "@HOURLY",
            "@annually",
            "-",
        ] {
            assert_eq!(check(expr), None, "{expr}");
        }
    }

    #[test]
    fn rejects_invalid_expressions() {
        for (expr, message) in [
            ("0 0 * * *", "this one has 5"),
            ("0 0 * * * * *", "this one has 7"),
            ("", "this one has 0"),
            ("60 * * * * *", "Cron second `60`: 60 is outside 0-59"),
            ("* 60 * * * *", "Cron minute `60`"),
            ("* * 24 * * *", "Cron hour `24`: 24 is outside 0-23"),
            ("* * * 0 * *", "Cron day-of-month `0`: 0 is outside 1-31"),
            ("* * * 32 * *", "outside 1-31"),
            ("* * * * 13 *", "Cron month `13`: 13 is outside 1-12"),
            ("* * * * 0 *", "outside 1-12"),
            ("* * * * * 8", "Cron day-of-week `8`: 8 is outside 0-7"),
            ("*/0 * * * * *", "must be 1 or more"),
            ("*/x * * * * *", "step `x` is not a number"),
            ("5-3 * * * * *", "range `5-3` ends before it starts"),
            ("0 0 0 * * SAT-MON", "ends before it starts"),
            ("? * * * * *", "`?` is only allowed"),
            ("* * * * FOO *", "not a number or a month name"),
            ("* * * * * FUNDAY", "not a number or a day name"),
            ("a * * * * *", "`a` is not a number"),
            ("0 0 0 1,,2 * *", "a value is missing"),
            ("0 0 0 L-31 * *", "offset from the last day must be 0-30"),
            ("0 0 0 0W * *", "0 is outside 1-31"),
            ("0 0 0 1,LW * *", "on its own"),
            ("0 0 0 ? * L", "needs a day before it"),
            ("0 0 0 ? * 1#6", "must be 1-5"),
            ("0 0 0 ? * #1", "a value is missing"),
            ("0 0 0 ? * 1#", "must be 1-5"),
            ("0 0 0 ? * 1L,2", "on its own"),
            ("@weekdays", "Unknown cron macro `@weekdays`"),
        ] {
            let found = check(expr).unwrap_or_else(|| panic!("{expr:?} should be invalid"));
            assert!(found.contains(message), "{expr:?}: {found}");
        }
    }
}
