//! Localization: JSON message catalogs (`include_str!`), zero runtime deps.
//!
//! Language selection precedence:
//!   1. `--lang <tag>` (global CLI flag)
//!   2. env `RECALL_LANG`
//!   3. `recall.config.json` → `"language"` (`"auto"` = OS locale)
//!   4. OS locale: `zh*` → Chinese, anything else → English
//!
//! Adding a language: add `<tag>.json` next to this file, add an enum variant
//! and an arm in `catalog()`. The parity test enforces identical key sets, and
//! the used-keys test scans call sites so nothing silently falls back to keys.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh-CN",
            Lang::En => "en",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Lang> {
        let t = tag.trim().to_ascii_lowercase();
        if t.starts_with("zh") {
            Some(Lang::Zh)
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

    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.map.get(key).map(String::as_str).unwrap_or(key)
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

    #[test]
    fn catalogs_have_identical_keys() {
        let zh = catalog(Lang::Zh);
        let en = catalog(Lang::En);
        let mut zh_keys: Vec<&str> = zh.keys().collect();
        let mut en_keys: Vec<&str> = en.keys().collect();
        zh_keys.sort();
        en_keys.sort();
        assert_eq!(zh_keys, en_keys, "zh-CN.json and en.json must share the same keys");
    }

    #[test]
    fn english_catalog_has_no_cjk() {
        for (k, v) in catalog(Lang::En)
            .keys()
            .map(|k| (k, catalog(Lang::En).get(k)))
        {
            assert!(
                !v.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "en.json value contains CJK characters: {k} = {v}"
            );
        }
    }

    /// Every `tr/trf/tr_in/trf_in` call site must use a key present in all catalogs.
    #[test]
    fn all_call_site_keys_exist() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut used: Vec<String> = Vec::new();
        collect_keys(&root, &mut used);
        assert!(!used.is_empty(), "no tr() calls found?");
        for key in &used {
            assert!(
                catalog(Lang::Zh).contains(key),
                "key missing in zh-CN.json: {key}"
            );
            assert!(catalog(Lang::En).contains(key), "key missing in en.json: {key}");
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
