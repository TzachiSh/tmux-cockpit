//! Checks contracts/ (#1020, #1052). Run by hand, from anywhere:
//! `cargo run --locked --manifest-path contracts/check/Cargo.toml [-- <contracts dir>]`.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";
const USAGE: &str = "usage: contracts-check [<contracts dir>]";

/// A JSON value read strictly. serde_json refuses NaN and Infinity on its own and keeps
/// the last of two equal keys; validators differ on which one wins, so a duplicate is refused.
struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<Value, E> {
        Ok(n.into())
    }
    fn visit_u64<E>(self, n: u64) -> Result<Value, E> {
        Ok(n.into())
    }
    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Value, E> {
        let n = serde_json::Number::from_f64(n).ok_or_else(|| E::custom("not a finite number"))?;
        Ok(Value::Number(n))
    }
    fn visit_str<E>(self, s: &str) -> Result<Value, E> {
        Ok(Value::String(s.to_owned()))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(Strict(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some((key, Strict(value))) = map.next_entry::<String, Strict>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key {key:?}")));
            }
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

fn parse_strict(text: &str) -> Result<Value, String> {
    serde_json::from_str::<Strict>(text)
        .map(|s| s.0)
        .map_err(|e| e.to_string())
}

fn load(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_strict(&text)
}

/// The path Python's jsonschema prints (`$.a[0].b`) for an instance pointer. A pointer
/// writes an index and a digits-only key alike, so the document says which one it is.
fn json_path(doc: &Value, pointer: &str) -> String {
    let mut out = String::from("$");
    let mut at = Some(doc);
    for raw in pointer.split('/').skip(1) {
        let seg = raw.replace("~1", "/").replace("~0", "~");
        if let Some(Value::Array(items)) = at {
            let _ = write!(out, "[{seg}]");
            at = seg.parse::<usize>().ok().and_then(|i| items.get(i));
        } else {
            out.push('.');
            out.push_str(&seg);
            at = at.and_then(|v| v.get(seg.as_str()));
        }
    }
    out
}

/// The README's rules a schema states about itself; empty when it keeps them all.
fn house_rules(schema: &Value) -> Vec<&'static str> {
    let mut bad = vec![];
    if schema.get("$schema").and_then(Value::as_str) != Some(DRAFT) {
        bad.push("$schema is not draft 2020-12");
    }
    let id = schema.get("$id").and_then(Value::as_str).unwrap_or("");
    if !id.starts_with("urn:tmux-cockpit:contracts:") {
        bad.push("$id is not urn:tmux-cockpit:contracts:<name>");
    }
    let version = schema.pointer("/properties/schema_version/const");
    if !version.is_some_and(|c| c.is_i64() || c.is_u64()) {
        bad.push("schema_version is not an integer const");
    }
    let required = schema.get("required").and_then(Value::as_array);
    if !required.is_some_and(|r| r.iter().any(|k| k == "schema_version")) {
        bad.push("schema_version is not required");
    }
    if schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        bad.push("additionalProperties is not false");
    }
    bad
}

/// The contracts folder: the one argument, else the folder holding this crate.
fn root_from(mut args: impl Iterator<Item = OsString>) -> Result<PathBuf, String> {
    let first = args.next();
    if args.next().is_some() {
        return Err(format!("{USAGE}: one argument at most"));
    }
    let Some(arg) = first else {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        return Ok(manifest.parent().unwrap_or(manifest).to_path_buf());
    };
    if arg.is_empty() || arg.to_string_lossy().starts_with('-') {
        return Err(format!("{USAGE}: <contracts dir> is {arg:?}, not a folder"));
    }
    Ok(PathBuf::from(arg))
}

/// The regex engine, not the default fancy-regex: no lookaround or backreference, and `$`
/// is the end of input, so a pattern needs no `(?!\n)` (#1052).
fn compile(schema: &Value) -> Result<jsonschema::Validator, String> {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .with_pattern_options(jsonschema::PatternOptions::regex())
        .build(schema)
        .map_err(|e| e.to_string())
}

/// A value as Python's f-string shows it: a string bare, an absent one as None.
fn shown(v: Option<&Value>) -> String {
    match v {
        None => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// One error a validator gave an example: its keyword, its path as `$.a[0]`, its message.
struct Refusal {
    keyword: String,
    at: String,
    message: String,
}

/// Loads one example and runs its schema over it; see `verdict` for what passes.
fn check_example(
    validator: &jsonschema::Validator,
    path: &Path,
    want: Option<&Value>,
) -> Result<(), String> {
    let doc = load(path).map_err(|e| format!("unreadable: {e}"))?;
    let refusals: Vec<Refusal> = validator
        .iter_errors(&doc)
        .map(|e| Refusal {
            keyword: e.kind().keyword().to_owned(),
            at: json_path(&doc, e.instance_path().as_str()),
            message: first_line(&e.to_string()).to_owned(),
        })
        .collect();
    verdict(&file_name(path), &refusals, want)
}

/// A valid-* example passes with no refusal. An invalid-* one passes with exactly one,
/// the keyword and path its errors.json entry `want` names, so it fails for its stated reason.
fn verdict(file: &str, refusals: &[Refusal], want: Option<&Value>) -> Result<(), String> {
    if file.starts_with("valid-") {
        return match refusals.first() {
            None => Ok(()),
            Some(r) => Err(format!("refused: {} at {}: {}", r.keyword, r.at, r.message)),
        };
    }
    let Some(want) = want else {
        return Err("no errors.json entry".into());
    };
    let (keyword, at) = (want.get("keyword"), want.get("at"));
    let named = |r: &Refusal| {
        Some(r.keyword.as_str()) == keyword.and_then(Value::as_str)
            && Some(r.at.as_str()) == at.and_then(Value::as_str)
    };
    match refusals {
        [] => Err("accepted".into()),
        [only] if named(only) => Ok(()),
        _ => {
            let listed: Vec<String> = refusals
                .iter()
                .map(|r| format!("('{}', '{}')", r.keyword, r.at))
                .collect();
            Err(format!(
                "refused for [{}], errors.json wants {} at {}",
                listed.join(", "),
                shown(keyword),
                shown(at)
            ))
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// The paths in `dir` whose file name passes `keep`, sorted; none when it cannot be read.
fn listed(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    let mut paths: Vec<PathBuf> = entries
        .map(|e| e.path())
        .filter(|p| keep(&file_name(p)))
        .collect();
    paths.sort();
    paths
}

/// Checks examples/<name>/ against its validator and returns how many examples it holds.
fn check_dir(
    root: &Path,
    name: &str,
    validator: &jsonschema::Validator,
    surprises: &mut Vec<String>,
) -> usize {
    let dir = root.join("examples").join(name);
    let files = listed(&dir, |n| n.ends_with(".json") && n != "errors.json");
    let why = match load(&dir.join("errors.json")) {
        Ok(Value::Object(map)) => map,
        Ok(_) => {
            surprises.push(format!("{name}/errors.json: not a JSON object"));
            Map::new()
        }
        Err(e) => {
            surprises.push(format!("{name}/errors.json: {e}"));
            Map::new()
        }
    };
    let mut invalid = BTreeSet::new();
    for path in &files {
        let file = file_name(path);
        if !(file.starts_with("valid-") || file.starts_with("invalid-")) {
            surprises.push(format!("{name}/{file}: not valid-* or invalid-*"));
            continue;
        }
        match check_example(validator, path, why.get(&file)) {
            Ok(()) => println!("ok   {name}/{file}"),
            Err(wrong) => {
                println!("FAIL {name}/{file}: {wrong}");
                surprises.push(format!("{name}/{file}: {wrong}"));
            }
        }
        if file.starts_with("invalid-") {
            invalid.insert(file);
        }
    }
    for kind in ["valid-", "invalid-"] {
        if !files.iter().any(|p| file_name(p).starts_with(kind)) {
            surprises.push(format!("{name}: no {kind}* example"));
        }
    }
    for stale in why.keys().filter(|k| !invalid.contains(*k)) {
        surprises.push(format!(
            "{name}/errors.json: names {stale}, which is no invalid-* file here"
        ));
    }
    files.len()
}

/// Prints one line per schema and example, then the summary; returns the surprises.
fn check(root: &Path) -> Vec<String> {
    let mut surprises = vec![];
    let schemas = listed(root, |n| n.ends_with(".schema.json"));
    let names: BTreeSet<String> = schemas
        .iter()
        .map(|p| file_name(p).trim_end_matches(".schema.json").to_string())
        .collect();
    let dirs: BTreeSet<String> = listed(&root.join("examples"), |_| true)
        .iter()
        .filter(|p| p.is_dir())
        .map(|p| file_name(p))
        .collect();
    for orphan in dirs.difference(&names) {
        surprises.push(format!("examples/{orphan}: no {orphan}.schema.json"));
    }
    let mut count = 0;
    for path in &schemas {
        let file = file_name(path);
        let name = file.trim_end_matches(".schema.json");
        let built = load(path).and_then(|schema| Ok((compile(&schema)?, schema)));
        let (validator, schema) = match built {
            Ok(pair) => pair,
            Err(e) => {
                println!("FAIL {file}: {}", first_line(&e));
                surprises.push(format!("{file}: {}", first_line(&e)));
                continue;
            }
        };
        let bad = house_rules(&schema);
        if bad.is_empty() {
            println!("ok   {file}");
        } else {
            println!("FAIL {file}: {}", bad.join("; "));
        }
        surprises.extend(bad.iter().map(|b| format!("{file}: {b}")));
        if !dirs.contains(name) {
            surprises.push(format!("{file}: no examples/{name} folder"));
            continue;
        }
        count += check_dir(root, name, &validator, &mut surprises);
    }
    if schemas.is_empty() {
        surprises.push("no *.schema.json in contracts/".into());
    }
    println!(
        "contracts: {} schemas, {count} examples, {} surprises",
        schemas.len(),
        surprises.len()
    );
    for s in &surprises {
        println!("  {s}");
    }
    surprises
}

fn main() -> ExitCode {
    let root = match root_from(std::env::args_os().skip(1)) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("contracts-check: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::read_dir(&root) {
        eprintln!(
            "contracts-check: cannot read the contracts folder {}: {e}",
            root.display()
        );
        return ExitCode::from(2);
    }
    if check(&root).is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_path_names_keys_and_indexes_as_python_does() {
        let doc = json!({"a": {"b": [10, {"c": 1}]}, "m": {"0": 1}});
        let rows = [
            ("root", "", "$"),
            ("keys", "/a/b", "$.a.b"),
            ("index", "/a/b/1/c", "$.a.b[1].c"),
            ("digit key", "/m/0", "$.m.0"),
        ];
        for (row, pointer, want) in rows {
            assert_eq!(json_path(&doc, pointer), want, "row {row}");
        }
    }

    #[test]
    fn json_path_unescapes_a_pointer_segment() {
        let doc = json!({"a/b": {"c~d": {"": 1}}});
        assert_eq!(json_path(&doc, "/a~1b/c~0d/"), "$.a/b.c~d.");
    }

    #[test]
    fn a_duplicate_key_is_refused_by_name() {
        let err = parse_strict(r#"{"schema_version": 1, "schema_version": 2}"#).unwrap_err();
        assert!(err.contains(r#"duplicate key "schema_version""#), "{err}");
    }

    #[test]
    fn a_duplicate_key_inside_an_array_is_refused() {
        let err = parse_strict(r#"{"xs": [{"a": 1}, {"b": 1, "b": 1}]}"#).unwrap_err();
        assert!(err.contains(r#"duplicate key "b""#), "{err}");
    }

    #[test]
    fn distinct_keys_parse_as_serde_json_does() {
        let text = r#"{"a": [1, -2, 2.5, "s", true, null], "b": {"a": {}}}"#;
        let want: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parse_strict(text), Ok(want));
    }

    #[test]
    fn nan_and_empty_input_are_refused() {
        for (row, text) in [("nan", "[NaN]"), ("infinity", "[Infinity]"), ("empty", "")] {
            assert!(parse_strict(text).is_err(), "row {row}");
        }
    }

    fn kept() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "urn:tmux-cockpit:contracts:x",
            "properties": {"schema_version": {"const": 1}},
            "required": ["schema_version"],
            "additionalProperties": false
        })
    }

    #[test]
    fn a_schema_keeping_every_rule_has_no_finding() {
        assert_eq!(house_rules(&kept()), Vec::<&str>::new());
    }

    #[test]
    fn schema_version_const_must_be_an_integer() {
        for (row, c) in [
            ("bool", json!(true)),
            ("float", json!(1.0)),
            ("string", json!("1")),
        ] {
            let mut s = kept();
            s["properties"]["schema_version"]["const"] = c;
            assert_eq!(
                house_rules(&s),
                ["schema_version is not an integer const"],
                "row {row}"
            );
        }
    }

    #[test]
    fn a_schema_that_is_no_object_breaks_every_rule() {
        assert_eq!(house_rules(&json!(true)).len(), 5);
    }

    #[test]
    fn lookaround_or_a_backreference_fails_to_compile() {
        let rows = [
            ("lookahead", r"^a(?!\n)$"),
            ("lookbehind", "(?<=a)b"),
            ("backref", r"^(a)\1$"),
        ];
        for (row, pattern) in rows {
            let schema = json!({"type": "string", "pattern": pattern});
            assert!(compile(&schema).is_err(), "row {row}");
        }
    }

    #[test]
    fn a_dollar_pattern_refuses_a_trailing_newline() {
        let v = compile(&json!({"type": "string", "pattern": "^%[0-9]{1,10}$"})).unwrap();
        assert!(v.is_valid(&json!("%12")));
        assert!(!v.is_valid(&json!("%12\n")));
    }

    fn args(list: &[&str]) -> impl Iterator<Item = OsString> {
        list.iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn no_argument_means_the_folder_holding_this_crate() {
        let root = root_from(args(&[])).unwrap();
        assert!(
            root.join("check").join("Cargo.toml").is_file(),
            "{}",
            root.display()
        );
    }

    #[test]
    fn one_argument_is_the_contracts_folder() {
        assert_eq!(
            root_from(args(&["some/dir"])),
            Ok(PathBuf::from("some/dir"))
        );
    }

    #[test]
    fn a_bad_argument_list_is_a_usage_error() {
        let rows = [
            ("two", &["a", "b"][..]),
            ("flag", &["--help"]),
            ("empty", &[""]),
        ];
        for (row, list) in rows {
            let err = root_from(args(list)).unwrap_err();
            assert!(err.starts_with(USAGE), "row {row}: {err}");
        }
    }
}
