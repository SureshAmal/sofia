
pub fn open(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("query is required".into());
    }
    if query.len() > 512 || query.chars().any(|c| c == '\n' || c == '\r') {
        return Err("query is invalid or too long".into());
    }
    let url = if query.starts_with("http://") || query.starts_with("https://") {
        query.to_string()
    } else {
        format!("https://duckduckgo.com/?q={}", urlencoding(query))
    };
    open::that(&url).map_err(|e| format!("The system browser could not open the URL: {e}"))?;
    Ok(url)
}
fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
