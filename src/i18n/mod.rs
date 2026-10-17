//! Localization: JSON message catalogs (`include_str!`), zero runtime deps.
//!
//! Language selection precedence:
//!   1. `--lang <tag>` (global CLI flag)
//!   2. env `RECALL_LANG`
//!   3. `recall.config.json` → `"language"` (`"auto"` = OS locale)
//!   4. OS locale: `zh*` → Chinese, `ja*` → Japanese, anything else → English
//!
//! `en.json` is the source of truth: every key used in the code must exist
//! there. Other catalogs may be partial — a missing key falls back to English
//! instead of leaking the raw key into the UI — so translations can land
//! incrementally. Tests enforce: en completeness, no unknown keys in other
//! catalogs, and a coverage floor.
//!
//! Adding a language: add `<tag>.json` next to this file, add an enum variant
//! and the arms in `code()` / `from_tag()` / `catalog()`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    Ja,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh-CN",
            Lang::Ja => "ja",
            Lang::En => "en",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Lang> {
        let t = tag.trim().to_ascii_lowercase();
        if t.starts_with("zh") {
            Some(Lang::Zh)
        } else if t.starts_with("ja") {
            Some(Lang::Ja)
        } else if t.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }

    /// OS locale (`zh-CN`, `en-US`, …); unknown → English.
    pub fn detect_os() -> Lang {
        sys_locale::get_locale()
            .as_deref()
            .and_then(Lang::from_tag)
            .unwrap_or(Lang::En)
    }

    /// Resolve using the documented precedence.
    pub fn resolve(cli: Option<&str>, config_path: &Path) -> Lang {
        if let Some(l) = cli.and_then(Lang::from_tag) {
            return l;
        }
        if let Ok(env) = std::env::var("RECALL_LANG") {
            if let Some(l) = Lang::from_tag(&env) {
                return l;
            }
        }
        if let Some(l) = language_from_config(config_path) {
            return l;
        }
        Lang::detect_os()
    }
}

#[derive(Debug)]
pub struct Catalog {
    map: BTreeMap<String, String>,
}

impl Catalog {
    fn parse(text: &str) -> Self {
        let map: BTreeMap<String, String> =
            serde_json::from_str(text).expect("i18n catalog must be valid JSON");
        Self { map }
    }

    /// Look up `key`, falling back to the English catalog. A partially
    /// translated locale therefore degrades to English instead of showing the
    /// raw key; only a key missing everywhere returns itself (a bug signal).
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(v) = self.map.get(key).map(String::as_str) {
            return v;
        }
        if let Some(v) = catalog(Lang::En).map.get(key).map(String::as_str) {
            return v;
        }
        key
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }
}

fn catalog(lang: Lang) -> &'static Catalog {
    match lang {
        Lang::Zh => {
            static C: OnceLock<Catalog> = OnceLock::new();
            C.get_or_init(|| Catalog::parse(include_str!("zh-CN.json")))
        }
        Lang::Ja => {
            static C: OnceLock<Catalog> = OnceLock::new();
            C.get_or_init(|| Catalog::parse(include_str!("ja.json")))
        }
        Lang::En => {
            static C: OnceLock<Catalog> = OnceLock::new();
            C.get_or_init(|| Catalog::parse(include_str!("en.json")))
        }
    }
}

static CURRENT: OnceLock<Lang> = OnceLock::new();

/// Set once at startup; later calls are ignored.
pub fn set_lang(lang: Lang) {
    let _ = CURRENT.set(lang);
}

pub fn current() -> Lang {
    *CURRENT.get().unwrap_or(&Lang::En)
}

/// Translate with the current language (set at startup).
pub fn tr<'a>(key: &'a str) -> &'a str {
    catalog(current()).get(key)
}

/// Translate explicitly for a given language.
pub fn tr_in<'a>(lang: Lang, key: &'a str) -> &'a str {
    catalog(lang).get(key)
}

/// Translate + replace `{0}`, `{1}`, … placeholders with the current language.
pub fn trf(key: &str, args: &[&str]) -> String {
    trf_in(current(), key, args)
}

pub fn trf_in(lang: Lang, key: &str, args: &[&str]) -> String {
    let mut s = catalog(lang).get(key).to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

fn language_from_config(path: &Path) -> Option<Lang> {
    #[derive(serde::Deserialize)]
    struct Cfg {
        language: Option<String>,
    }
    let text = std::fs::read_to_string(path).ok()?;
    let cfg: Cfg = serde_json::from_str(&text).ok()?;
    let tag = cfg.language?;
    if tag.eq_ignore_ascii_case("auto") {
        Some(Lang::detect_os())
    } else {
        Lang::from_tag(&tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All catalogs except English, for the cross-catalog checks below.
    const TRANSLATIONS: [Lang; 2] = [Lang::Zh, Lang::Ja];

    /// `en.json` is the source of truth: every call-site key must exist there.
    #[test]
    fn all_call_site_keys_exist_in_english() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut used: Vec<String> = Vec::new();
        collect_keys(&root, &mut used);
        assert!(!used.is_empty(), "no tr() calls found?");
        for key in &used {
            assert!(
                catalog(Lang::En).contains(key),
                "key missing in en.json: {key}"
            );
        }
    }

    /// Translations may be partial, but must not invent keys (that is a typo).
    #[test]
    fn translations_have_no_unknown_keys() {
        let en = catalog(Lang::En);
        for lang in TRANSLATIONS {
            for key in catalog(lang).keys() {
                assert!(
                    en.contains(key),
                    "{} has a key that does not exist in en.json: {key}",
                    lang.code()
                );
            }
        }
    }

    /// Guard against a language landing with a handful of keys; the missing
    /// ones are printed so a contributor sees the remaining to-do list.
    #[test]
    fn translations_cover_most_of_english() {
        let en = catalog(Lang::En);
        let total = en.keys().count();
        for lang in TRANSLATIONS {
            let cat = catalog(lang);
            let missing: Vec<&str> = en.keys().filter(|k| !cat.contains(k)).collect();
            let covered = total - missing.len();
            println!("{}: {covered}/{total} keys translated", lang.code());
            assert!(
                covered * 100 / total >= 80,
                "{} covers only {covered}/{total}; missing: {missing:?}",
                lang.code()
            );
        }
    }

    /// A gap in a translation must degrade to English, never to the raw key.
    #[test]
    fn missing_key_falls_back_to_english() {
        let mut map: BTreeMap<String, String> = serde_json::from_str(include_str!("en.json"))
            .expect("en.json must parse");
        map.remove("common.no_title");
        let partial = Catalog { map };
        assert_eq!(partial.get("common.no_title"), "(untitled)");
        assert_eq!(partial.get("totally.unknown.key"), "totally.unknown.key");
    }

    #[test]
    fn english_catalog_has_no_cjk() {
        let en = catalog(Lang::En);
        for (k, v) in en.keys().map(|k| (k, en.get(k))) {
            assert!(
                !v.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "en.json value contains CJK characters: {k} = {v}"
            );
        }
    }

    /// Simplified-Chinese-only characters never appear in Japanese: catching
    /// them here catches a copy-pasted zh value in ja.json.
    #[test]
    fn japanese_catalog_has_no_simplified_chinese_forms() {
        const SIMPLIFIED_ONLY: &str = "们么让说请谢东车还过进这录简单认识复杂铁银见时间无头后从儿习书买义乐乡举兴为个对发关开长门问题试页选择图标签码实应结给线";
        let ja = catalog(Lang::Ja);
        for (k, v) in ja.keys().map(|k| (k, ja.get(k))) {
            if let Some(c) = v.chars().find(|c| SIMPLIFIED_ONLY.contains(*c)) {
                panic!("ja.json value looks like Simplified Chinese ({c}): {k} = {v}");
            }
        }
    }

    fn collect_keys(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_keys(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                for marker in [
                    "trf_in(lang, \"",
                    "tr_in(lang, \"",
                    "trf_in(self.lang, \"",
                    "tr_in(self.lang, \"",
                    "trf_in(Lang::Zh, \"",
                    "tr_in(Lang::Zh, \"",
                    "trf(\"",
                    "tr(\"",
                ] {
                    let mut hay = text.as_str();
                    while let Some(i) = hay.find(marker) {
                        let rest = &hay[i + marker.len()..];
                        if let Some(end) = rest.find('"') {
                            let key = &rest[..end];
                            if !key.is_empty()
                                && key
                                    .chars()
                                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-')
                            {
                                out.push(key.to_string());
                            }
                        }
                        hay = &hay[i + marker.len()..];
                    }
                }
            }
        }
    }
}
