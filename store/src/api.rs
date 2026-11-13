//! The catalog: Flathub's public API. Flathub only lists real desktop apps, so there are
//! no libraries or dev packages here, and we drop anything that isn't a desktop app.

use std::{sync::OnceLock, time::Duration};

use serde_json::Value;

/// The app we put front and centre.
pub const FEATURED_ID: &str = "net.leatherclient.LeatherClient";

const BASE: &str = "https://flathub.org/api/v2";
const USER_AGENT: &str = "window-shopping/0.2 (VIDE-DE)";

/// (Flathub's name, what we call it).
pub const CATEGORIES: [(&str, &str); 10] = [
    ("Office", "Productivity"),
    ("Graphics", "Photo & design"),
    ("AudioVideo", "Audio & video"),
    ("Development", "Developer tools"),
    ("Education", "Education"),
    ("Network", "Internet"),
    ("Science", "Science & maths"),
    ("System", "System tools"),
    ("Utility", "Utilities"),
    ("Game", "Games"),
];

pub fn category_label(raw: &str) -> String {
    CATEGORIES
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(raw))
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "Apps".into())
}

#[derive(Clone, Debug, Default)]
pub struct App {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub developer: String,
    pub icon: String,
    pub category: String,
}

#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub app: App,
    pub description: String,
    pub screenshots: Vec<String>,
    pub license: String,
    pub categories: Vec<String>,
    pub version: String,
    pub released: String,
    pub notes: String,
    pub homepage: String,
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(25)))
            .build()
            .into()
    })
}

pub fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut resp = agent().get(url).header("User-Agent", USER_AGENT).call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(32 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())
}

fn get_json(path: &str) -> Result<Value, String> {
    let mut resp = agent()
        .get(&format!("{BASE}{path}"))
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn hits(v: &Value) -> Vec<App> {
    v.get("hits").and_then(Value::as_array).map(|a| a.iter().filter_map(parse_app).collect()).unwrap_or_default()
}

/// `popular`, `trending` or `recently-updated`.
pub fn collection(kind: &str, page: u32, per_page: u32) -> Result<Vec<App>, String> {
    Ok(hits(&get_json(&format!("/collection/{kind}?page={page}&per_page={per_page}"))?))
}

pub fn category(cat: &str, page: u32, per_page: u32) -> Result<Vec<App>, String> {
    Ok(hits(&get_json(&format!("/collection/category/{cat}?page={page}&per_page={per_page}"))?))
}

pub fn search(query: &str) -> Result<Vec<App>, String> {
    let body = serde_json::json!({ "query": query, "hits_per_page": 48, "filters": [] }).to_string();
    let mut resp = agent()
        .post(&format!("{BASE}/search"))
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|e| e.to_string())?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok(hits(&serde_json::from_str(&text).map_err(|e| e.to_string())?))
}

pub fn detail(id: &str) -> Result<Detail, String> {
    let v = get_json(&format!("/appstream/{id}"))?;
    if v.get("type").and_then(Value::as_str).is_some_and(|t| t != "desktop-application") {
        return Err("not an app".into());
    }
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let categories: Vec<String> = v
        .get("categories")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let category = CATEGORIES
        .iter()
        .find(|(k, _)| categories.iter().any(|c| c == k))
        .map(|(k, _)| k.to_string())
        .unwrap_or_default();
    let release = v.get("releases").and_then(Value::as_array).and_then(|r| r.first());
    let free = v.get("is_free_license").and_then(Value::as_bool).unwrap_or(false);
    let license = text("project_license");
    Ok(Detail {
        app: App {
            id: id.to_string(),
            name: text("name"),
            summary: text("summary"),
            developer: text("developer_name"),
            icon: text("icon"),
            category,
        },
        description: html_to_text(&text("description")),
        screenshots: v.get("screenshots").and_then(Value::as_array).map(|a| a.iter().filter_map(screenshot_url).collect()).unwrap_or_default(),
        license: license_label(&license, free),
        categories,
        version: release.and_then(|r| r.get("version")).and_then(Value::as_str).unwrap_or_default().to_string(),
        released: release.and_then(|r| number(r.get("timestamp")?)).map(|t| format_date(t as i64)).unwrap_or_default(),
        notes: release.and_then(|r| r.get("description")).and_then(Value::as_str).map(html_to_text).unwrap_or_default(),
        homepage: v.get("urls").and_then(|u| u.get("homepage")).and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

fn license_label(raw: &str, free: bool) -> String {
    if raw.is_empty() {
        "Not listed".into()
    } else if free {
        raw.to_string()
    } else {
        "Proprietary".into()
    }
}

fn number(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// The screenshot size nearest 752 px wide: sharp in our window without a huge download.
fn screenshot_url(shot: &Value) -> Option<String> {
    let sizes = shot.get("sizes")?.as_array()?;
    sizes
        .iter()
        .filter_map(|s| Some((number(s.get("width")?)?, s.get("src")?.as_str()?)))
        .min_by_key(|(w, _)| (*w as i64 - 752).abs())
        .map(|(_, src)| src.to_string())
}

fn parse_app(v: &Value) -> Option<App> {
    // Search hits sometimes omit the type; when it's there it must be a desktop app.
    if v.get("type").and_then(Value::as_str).is_some_and(|t| t != "desktop-application") {
        return None;
    }
    let id = v.get("app_id").and_then(Value::as_str)?.to_string();
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let name = text("name");
    if id.is_empty() || name.is_empty() {
        return None;
    }
    let category = match v.get("main_categories") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.first().and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    Some(App { id, name, summary: text("summary"), developer: text("developer_name"), icon: text("icon"), category })
}

/// Appstream descriptions are a little HTML (`<p>`, `<ul>`); show them as plain text.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    let mut last_space = true;
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if in_tag {
            if c != '>' {
                tag.push(c);
                continue;
            }
            in_tag = false;
            match tag.trim().to_ascii_lowercase().as_str() {
                "li" => out.push_str("\n• "),
                "/p" | "/ul" | "/ol" => out.push_str("\n\n"),
                "br" | "br/" | "br /" => out.push('\n'),
                _ => continue,
            }
            last_space = true;
        } else if c == '<' {
            in_tag = true;
            tag.clear();
        } else if c == '&' {
            let mut ent = String::new();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == ';' || ent.len() > 8 {
                    break;
                }
                ent.push(n);
            }
            let decoded = match ent.as_str() {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" | "#39" => '\'',
                "nbsp" => ' ',
                _ => '?',
            };
            out.push(decoded);
            last_space = decoded == ' ';
        } else if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    let mut lines: Vec<&str> = out.lines().map(str::trim).collect();
    // Collapse runs of blank lines to one.
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().to_string()
}

/// "Nov 12, 2026" from a unix timestamp (civil-from-days, so no date crate).
pub fn format_date(ts: i64) -> String {
    let z = ts.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {day}, {year}", MONTHS[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_plain_text() {
        let t = html_to_text("<p>Hello  &amp; welcome.</p>\n   <ul><li>One</li><li>Two</li></ul>");
        assert_eq!(t, "Hello & welcome.\n\n• One\n• Two");
    }

    #[test]
    fn dates() {
        assert_eq!(format_date(1_789_603_200), "Sep 17, 2026");
        assert_eq!(format_date(0), "Jan 1, 1970");
    }
}
