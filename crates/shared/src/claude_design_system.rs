//! Migrates a Claude design system: the .zip its page downloads becomes one DESIGN.md. Only text is read and nothing
//! is written to disk or run, so paths and previews in the archive can't do anything. Format: Claude's Design System
//! type (README.md, tokens.json, design-system.json, components/<Name>/README.md).
//! Port of `packages/shared/src/claude-design-system.ts`.

use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::sync::LazyLock;

/// How much of a workspace system's DESIGN.md agents are handed (`artifact_guide`).
pub const DESIGN_MD_LIMIT: usize = 32000;
const MAX_FILE: u64 = 2_000_000;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(TEXT, r"(?i)\.(md|json)$");
re!(WS_NL, r"\s*\n\s*");
re!(LEADING_H1, r"^\s*#\s+.*\n?");
re!(FENCE, r"^\s*(```|~~~)");
re!(HEADING_1_5, r"^#{1,5}\s");
re!(ALIAS, r"^\{(.+)\}$");
re!(MARKER, r"(?i)(^|/)(design-system\.json|tokens\.json|README\.md)$");
re!(README_H1, r"(?m)^#\s+(.+)$");
re!(BLANK_LINE, r"\n\s*\n");
re!(PLAIN_NOT, r"^[#|<>\-*`]");
re!(MD_FILE, r"(?i)\.md$");
re!(README_ROOT, r"(?i)^readme\.md$");
re!(SKIP_DIRS, r"^(components|archived|api|assets)/");
re!(COMPONENT_README, r"^components/([^/]+)/README\.md$");
re!(API_ARCHIVED, r"^(api|archived)/");
re!(GENERATED, r"^(tokens\.css|manifest\.json|SKILL\.md|index\.html)$");
re!(PREVIEW, r"^components/[^/]+/preview\.html$");
re!(FONTS, r"^fonts/");
re!(ASSETS, r"^assets/");
re!(CODE_FILES, r"^components/(bundle\.|lib/|index\.d\.ts)");
re!(CAMEL, r"([a-z])([A-Z])");
re!(WS, r"\s+");

const TOO_LARGE: &str = "This .zip has a file too large for a design system";
const DAMAGED: &str = "This .zip is damaged";

fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(data).take(MAX_FILE + 1).read_to_end(&mut out).map_err(|_| DAMAGED.to_string())?;
    if out.len() as u64 > MAX_FILE {
        return Err(TOO_LARGE.into());
    }
    Ok(out)
}

/// The text files of an archive by path, and the paths of everything else.
#[derive(Debug, Default, Clone)]
pub struct Zip {
    pub text: BTreeMap<String, String>,
    pub other: Vec<String>,
}

fn u16_at(b: &[u8], o: usize) -> Result<usize, String> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize).ok_or_else(|| DAMAGED.to_string())
}
fn u32_at(b: &[u8], o: usize) -> Result<usize, String> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize).ok_or_else(|| DAMAGED.to_string())
}

pub fn read_zip(bytes: &[u8]) -> Result<Zip, String> {
    let mut end = bytes.len() as isize - 22;
    let stop = (end - 65535).max(0);
    while end >= stop && u32_at(bytes, end as usize)? != 0x06054b50 {
        end -= 1;
    }
    if end < stop {
        return Err("This file is not a .zip".into());
    }
    let end = end as usize;
    let count = u16_at(bytes, end + 10)?;
    let mut at = u32_at(bytes, end + 16)?;
    if count == 0xffff || at == 0xffff_ffff {
        return Err("This .zip is too large to be a design system".into());
    }
    let mut zip = Zip::default();
    for _ in 0..count {
        if at + 46 > bytes.len() || u32_at(bytes, at)? != 0x02014b50 {
            return Err(DAMAGED.into());
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let packed = u32_at(bytes, at + 20)?;
        let name_len = u16_at(bytes, at + 28)?;
        let local = u32_at(bytes, at + 42)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len).ok_or(DAMAGED)?).into_owned();
        at += 46 + name_len + u16_at(bytes, at + 30)? + u16_at(bytes, at + 32)?;
        if name.ends_with('/') {
            continue;
        }
        if !TEXT.is_match(&name) {
            zip.other.push(name);
            continue;
        }
        if flags & 1 != 0 {
            return Err("This .zip is password protected".into());
        }
        if method != 0 && method != 8 {
            return Err("This .zip uses a compression method WorkspaceAI can’t read".into());
        }
        if local + 30 > bytes.len() {
            return Err(DAMAGED.into());
        }
        let start = local + 30 + u16_at(bytes, local + 26)? + u16_at(bytes, local + 28)?;
        if start + packed > bytes.len() {
            return Err(DAMAGED.into());
        }
        if method == 0 && packed as u64 > MAX_FILE {
            return Err(TOO_LARGE.into());
        }
        let raw = &bytes[start..start + packed];
        let data = if method == 8 { inflate(raw)? } else { raw.to_vec() };
        zip.text.insert(name, String::from_utf8_lossy(&data).into_owned());
    }
    Ok(zip)
}

const NULL: Value = Value::Null;
fn field<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or(&NULL)
}

/// JS `String(v ?? '')`.
fn js_str(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Value::Array(a) => a.iter().map(|x| js_str(x)).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JS truthiness for the `a || b` name chain.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

fn cell(v: &Value) -> String {
    WS_NL.replace_all(&js_str(v), " ").replace('|', "\\|")
}
fn row(cells: &[Value]) -> String {
    format!("| {} |", cells.iter().map(cell).collect::<Vec<_>>().join(" | "))
}
fn table(head: &[&str], rows: Vec<Vec<Value>>) -> String {
    let h: Vec<Value> = head.iter().map(|s| Value::String(s.to_string())).collect();
    let sep: Vec<Value> = head.iter().map(|_| Value::String("---".into())).collect();
    let mut lines = vec![row(&h), row(&sep)];
    lines.extend(rows.iter().map(|r| row(r)));
    lines.join("\n")
}
fn list(v: &Value) -> Vec<&Value> {
    field(v, "tokens").as_array().map(|a| a.iter().collect()).unwrap_or_default()
}
fn title(s: &str) -> String {
    let s = CAMEL.replace_all(s, "$1 $2");
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
fn s(x: &str) -> Value {
    Value::String(x.into())
}

fn colours(color: &Value) -> String {
    let tokens = list(color);
    if tokens.is_empty() {
        return String::new();
    }
    let given = field(color, "themes").as_array().filter(|a| !a.is_empty());
    let themes: Vec<Value> = match given {
        Some(a) => a.clone(),
        None => vec![serde_json::json!({ "id": "value", "name": "Value" })],
    };
    let first_id = js_str(field(&themes[0], "id"));
    let by_name: HashMap<String, &Value> = tokens.iter().map(|t| (js_str(field(t, "name")), *t)).collect();
    fn value(t: Option<&Value>, theme: &str, first: &str, by_name: &HashMap<String, &Value>, depth: usize) -> String {
        let v = t.map(|t| field(t, "value")).unwrap_or(&NULL);
        let raw: Value = match v {
            Value::String(_) => v.clone(),
            Value::Object(o) => {
                let a = o.get(theme).filter(|x| !x.is_null());
                a.or_else(|| o.get(first)).cloned().unwrap_or(Value::Null)
            }
            _ => Value::Null,
        };
        let text = js_str(&raw);
        if let Some(a) = ALIAS.captures(&text) {
            if depth < 16 {
                return value(by_name.get(&a[1]).copied(), theme, first, by_name, depth + 1);
            }
        }
        text
    }
    let mut head: Vec<Value> = vec![s("Token")];
    head.extend(themes.iter().map(|t| if truthy(field(t, "name")) { field(t, "name").clone() } else { field(t, "id").clone() }));
    head.push(s("Use"));
    let head_s: Vec<String> = head.iter().map(js_str).collect();
    let head_r: Vec<&str> = head_s.iter().map(|x| x.as_str()).collect();
    let rows = tokens
        .iter()
        .map(|t| {
            let mut r = vec![field(t, "name").clone()];
            r.extend(themes.iter().map(|m| Value::String(value(Some(t), &js_str(field(m, "id")), &first_id, &by_name, 0))));
            r.push(field(t, "usage").clone());
            r
        })
        .collect();
    format!("## Colour tokens\n\n{}", table(&head_r, rows))
}

fn typography(ty: &Value) -> String {
    if ty.is_null() {
        return String::new();
    }
    let families: Vec<Vec<Value>> = field(ty, "families").as_object().map(|o| o.iter().map(|(k, v)| vec![s(k), v.clone()]).collect()).unwrap_or_default();
    let mut styles: Vec<Vec<Value>> = Vec::new();
    for g in field(ty, "groups").as_array().into_iter().flatten() {
        for st in field(g, "styles").as_array().into_iter().flatten() {
            let family = if truthy(field(st, "family")) { field(st, "family") } else { field(g, "family") };
            styles.push(["name", "family", "fontSize", "lineHeight", "fontWeight", "letterSpacing", "usage"].iter().map(|k| if *k == "family" { family.clone() } else { field(st, k).clone() }).collect());
        }
    }
    let mut parts = Vec::new();
    if !families.is_empty() {
        parts.push(table(&["Family", "Stack"], families));
    }
    if !styles.is_empty() {
        parts.push(table(&["Style", "Family", "Size", "Line height", "Weight", "Tracking", "Use"], styles));
    }
    if parts.is_empty() { String::new() } else { format!("## Type\n\n{}", parts.join("\n\n")) }
}

fn per_theme(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::String(o.iter().map(|(k, x)| format!("{k}: {}", js_str(x))).collect::<Vec<_>>().join("; ")),
        _ => v.clone(),
    }
}

/// Drop a document's leading H1, and move its other headings down `by` levels.
fn section(md: &str, by: usize) -> String {
    let body = LEADING_H1.replace(md, "");
    let mut fenced = false;
    body.split('\n')
        .map(|l| {
            if FENCE.is_match(l) {
                fenced = !fenced;
            }
            if !fenced && HEADING_1_5.is_match(l) { format!("{}{l}", "#".repeat(by)) } else { l.to_string() }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct MigratedSystem {
    pub title: String,
    pub markdown: String,
    pub not_carried: Vec<String>,
}

/// One Claude design system as a DESIGN.md: tokens first, since agents read only the start of a long one.
pub fn claude_system_to_design_md(zip: &Zip, fallback_name: &str) -> Result<MigratedSystem, String> {
    let not_ds = "This .zip is not a Claude design system: it has no README.md or tokens.json";
    let mut roots: Vec<String> = zip.text.keys().filter(|p| MARKER.is_match(p)).map(|p| p[..p.rfind('/').map_or(0, |i| i + 1)].to_string()).collect();
    roots.sort_by_key(|r| r.encode_utf16().count());
    let root = roots.into_iter().next().ok_or(not_ds)?;
    let files: BTreeMap<String, &String> = zip.text.iter().filter(|(p, _)| p.starts_with(&root)).map(|(p, t)| (p[root.len()..].to_string(), t)).collect();
    let json = |name: &str| -> Result<Value, String> {
        match files.get(name) {
            None => Ok(Value::Null),
            Some(raw) => serde_json::from_str(raw).map_err(|_| format!("This design system’s {name} is not valid JSON")),
        }
    };
    let tokens = json("tokens.json")?;
    let index = json("design-system.json")?;
    let readme = files.get("README.md").or_else(|| files.get("readme.md")).map(|s| s.as_str()).unwrap_or("");
    if !truthy(&tokens) && readme.trim().is_empty() {
        return Err(not_ds.into());
    }
    let raw_name = [field(&index, "title").clone(), field(&tokens, "name").clone(), README_H1.captures(readme).map_or(Value::Null, |c| s(&c[1])), s(fallback_name)]
        .into_iter()
        .find(truthy)
        .unwrap();
    let name: String = WS.replace_all(&js_str(&raw_name), " ").trim().chars().take(120).collect();

    let first_para = |md: &str| BLANK_LINE.split(md).next().unwrap_or("").trim().to_string();
    let summary = first_para(&section(readme, 0));
    let plain = !summary.is_empty() && !PLAIN_NOT.is_match(&summary);
    let family_keys: Vec<&String> = tokens.as_object().map(|o| o.keys().filter(|k| !["color", "type"].contains(&k.as_str()) && !list(&o[*k]).is_empty()).collect()).unwrap_or_default();
    let mut sections: Vec<&String> = files.keys().filter(|p| MD_FILE.is_match(p) && !README_ROOT.is_match(p) && !SKIP_DIRS.is_match(p)).collect();
    sections.sort();
    let mut components: Vec<(String, &String)> = files.iter().filter_map(|(p, md)| COMPONENT_README.captures(p).map(|m| (m[1].to_string(), *md))).collect();
    components.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then(a.0.cmp(&b.0)));

    let left: Vec<&str> = zip.other.iter().filter(|p| p.starts_with(&root)).map(|p| &p[root.len()..]).filter(|p| !API_ARCHIVED.is_match(p) && !GENERATED.is_match(p)).collect();
    let count = |re: &Regex| left.iter().filter(|p| re.is_match(p)).count();
    let not_carried: Vec<String> = [(count(&PREVIEW), "component preview"), (count(&FONTS), "font file"), (count(&ASSETS), "logo or image file"), (count(&CODE_FILES), "component code file")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}{}", if n == 1 { "" } else { "s" }))
        .collect();

    let book = section(readme, 1).replacen(if plain { summary.as_str() } else { "\0" }, "", 1).trim().to_string();
    let mut parts: Vec<String> = vec![format!("# {name}")];
    if plain {
        parts.push(format!("> {}", WS_NL.replace_all(&summary, " ")));
    }
    parts.push(colours(field(&tokens, "color")));
    parts.push(typography(field(&tokens, "type")));
    for k in family_keys {
        let rows = list(&tokens[k.as_str()]).iter().map(|t| vec![field(t, "name").clone(), per_theme(field(t, "value")), field(t, "usage").clone()]).collect();
        parts.push(format!("## {}\n\n{}", title(k), table(&["Token", "Value", "Use"], rows)));
    }
    if !book.is_empty() {
        parts.push(format!("## Brand book\n\n{book}"));
    }
    parts.extend(sections.iter().map(|p| section(files[*p], 1)));
    if !components.is_empty() {
        parts.push(format!("## Components\n\n{}", components.iter().map(|(c, md)| format!("### {c}\n\n{}", section(md, 2))).collect::<Vec<_>>().join("\n\n")));
    }
    if !not_carried.is_empty() {
        parts.push(format!("## Not carried over\n\nMigrated from a Claude design system, as text. Still in Claude: {}. Don’t approximate a logo or a licensed font: set the name in plain type and use the fallbacks in the font stacks above.", not_carried.join(", ")));
    }
    let markdown = parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n") + "\n";
    Ok(MigratedSystem { title: name, markdown, not_carried })
}
