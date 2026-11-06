//! The web profiler's request list: `var/cache/dev/profiler/index.csv`, one
//! line per request (token, ip, method, url, time, parent, status code).

pub const INDEX_FILE: &str = "var/cache/dev/profiler/index.csv";
pub const MAX_ROWS: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub token: String,
    pub ip: String,
    pub method: String,
    pub url: String,
    pub time: i64,
    pub status: String,
}

/// `YYYY-MM-DD HH:MM:SS` in UTC.
pub fn timestamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Fields of one CSV line, with `"…"` quoting.
fn fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().unwrap().push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }

    fields
}

/// The latest requests first, at most `MAX_ROWS`; sub-requests (with a parent) are left out.
pub fn rows(text: &str) -> Vec<Request> {
    let mut requests: Vec<Request> = text
        .lines()
        .filter_map(|line| {
            let [token, ip, method, url, time, parent, status] = <[String; 7]>::try_from(fields(line)).ok()?;
            if !parent.is_empty() || token.is_empty() {
                return None;
            }

            Some(Request { token, ip, method, url, time: time.trim().parse().ok()?, status })
        })
        .collect();

    requests.sort_by(|a, b| b.time.cmp(&a.time));
    requests.truncate(MAX_ROWS);
    requests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiler_index_rows_parse() {
        let text = "aaa111,127.0.0.1,GET,http://localhost/blog,1700000000,,200\n\
                    bbb222,10.0.0.2,POST,\"http://localhost/search?a=1,2\",1700000100,,302\n\
                    ccc333,127.0.0.1,GET,http://localhost/_fragment,1700000101,bbb222,200\n\
                    \n\
                    broken line\n";
        let found = rows(text);
        let tokens: Vec<_> = found.iter().map(|r| r.token.as_str()).collect();
        assert_eq!(tokens, ["bbb222", "aaa111"]);
        assert_eq!(found[0].url, "http://localhost/search?a=1,2");
        assert_eq!((found[0].method.as_str(), found[0].status.as_str(), found[0].time), ("POST", "302", 1700000100));

        let many: String = (0..150).map(|i| format!("t{i},ip,GET,/u{i},{},,200\n", 1000 + i)).collect();
        let capped = rows(&many);
        assert_eq!(capped.len(), MAX_ROWS);
        assert_eq!(capped[0].token, "t149");

        assert_eq!(timestamp(1700000000), "2023-11-14 22:13:20");
    }
}
