//! Languages. Every text lives in `locales/<code>.json` (one flat map of key → text), embedded in the exe so Ninho
//! stays a single portable file. A text missing in a language falls back to English, then to "?".
//! To add a language: copy `locales/en.json`, translate the values, and add a line to LOCALES.
use std::collections::HashMap;
use std::sync::OnceLock;

use eframe::egui;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Writing systems that need a font egui does not ship; Windows has them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Japanese,
    Korean,
    ChineseSimplified,
    ChineseTraditional,
    Devanagari,
}

/// (code, name in that language, texts, writing system). English first: it is the fallback.
const LOCALES: &[(&str, &str, &str, Script)] = &[
    ("en", "English", include_str!("../locales/en.json"), Script::Latin),
    ("pt-BR", "Português (Brasil)", include_str!("../locales/pt-BR.json"), Script::Latin),
    ("es", "Español", include_str!("../locales/es.json"), Script::Latin),
    ("fr", "Français", include_str!("../locales/fr.json"), Script::Latin),
    ("de", "Deutsch", include_str!("../locales/de.json"), Script::Latin),
    ("it", "Italiano", include_str!("../locales/it.json"), Script::Latin),
    ("nl", "Nederlands", include_str!("../locales/nl.json"), Script::Latin),
    ("pl", "Polski", include_str!("../locales/pl.json"), Script::Latin),
    ("ru", "Русский", include_str!("../locales/ru.json"), Script::Latin),
    ("uk", "Українська", include_str!("../locales/uk.json"), Script::Latin),
    ("tr", "Türkçe", include_str!("../locales/tr.json"), Script::Latin),
    ("id", "Bahasa Indonesia", include_str!("../locales/id.json"), Script::Latin),
    ("vi", "Tiếng Việt", include_str!("../locales/vi.json"), Script::Latin),
    ("ja", "日本語", include_str!("../locales/ja.json"), Script::Japanese),
    ("ko", "한국어", include_str!("../locales/ko.json"), Script::Korean),
    ("zh-CN", "简体中文", include_str!("../locales/zh-CN.json"), Script::ChineseSimplified),
    ("zh-TW", "繁體中文", include_str!("../locales/zh-TW.json"), Script::ChineseTraditional),
    ("hi", "हिन्दी", include_str!("../locales/hi.json"), Script::Devanagari),
];

/// A language: an index into LOCALES. Saved in ninho.json as its code ("pt-BR"); the old "Pt" / "En" still read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lang(usize);

impl Lang {
    pub fn all() -> impl Iterator<Item = Lang> {
        (0..LOCALES.len()).map(Lang)
    }
    pub fn code(self) -> &'static str {
        LOCALES[self.0].0
    }
    pub fn label(self) -> &'static str {
        LOCALES[self.0].1
    }
    /// A code like "pt-BR", "pt-PT", "es-MX", "zh-HK": the exact language, else the same language, else English.
    pub fn from_code(code: &str) -> Lang {
        let code = code.trim();
        let low = code.to_ascii_lowercase();
        if let Some(i) = LOCALES.iter().position(|l| l.0.eq_ignore_ascii_case(code)) {
            return Lang(i);
        }
        let pick = if low.starts_with("zh-hant") || low.starts_with("zh-tw") || low.starts_with("zh-hk") || low.starts_with("zh-mo") {
            "zh-TW"
        } else if low.starts_with("zh") {
            "zh-CN"
        } else if low.starts_with("pt") {
            "pt-BR"
        } else {
            low.split(['-', '_']).next().unwrap_or("en")
        };
        LOCALES.iter().position(|l| l.0.eq_ignore_ascii_case(pick)).map(Lang).unwrap_or(Lang(0))
    }
    /// The language Windows shows its menus in (first start).
    pub fn system() -> Lang {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
            }
            let mut buf = [0u16; 85];
            let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
            if n > 1 {
                return Lang::from_code(&String::from_utf16_lossy(&buf[..(n - 1) as usize]));
            }
        }
        Lang(0)
    }
}

impl Serialize for Lang {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Lang {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Lang, D::Error> {
        let v = String::deserialize(d)?;
        Ok(match v.as_str() {
            // Settings written by Ninho 0.1 – 0.4.
            "Pt" => Lang::from_code("pt-BR"),
            "En" => Lang(0),
            other => Lang::from_code(other),
        })
    }
}

fn tables() -> &'static Vec<HashMap<String, &'static str>> {
    static T: OnceLock<Vec<HashMap<String, &'static str>>> = OnceLock::new();
    T.get_or_init(|| {
        LOCALES
            .iter()
            .map(|(code, _, json, _)| {
                let map: HashMap<String, String> = serde_json::from_str(json).unwrap_or_else(|e| {
                    eprintln!("locales/{code}.json: {e}");
                    HashMap::new()
                });
                // Leaked once at start: the texts live as long as the program.
                map.into_iter().map(|(k, v)| (k, &*Box::leak(v.into_boxed_str()))).collect()
            })
            .collect()
    })
}

pub fn tr(lang: Lang, key: &str) -> &'static str {
    let t = tables();
    t.get(lang.0).and_then(|m| m.get(key)).or_else(|| t[0].get(key)).copied().unwrap_or("?")
}

/// Texts that mark an activity line as a problem or a success, in each language: every text whose English says
/// "could not", "failed" or "in use" (problem) or "started" or "installed" (success), as that language writes it.
/// The activity list colors a line by them, whatever language Ninho is in.
pub fn moods(lang: Lang) -> &'static (Vec<String>, Vec<String>) {
    static M: OnceLock<Vec<(Vec<String>, Vec<String>)>> = OnceLock::new();
    let all = M.get_or_init(|| {
        let t = tables();
        t.iter()
            .map(|m| {
                let (mut bad, mut good) = (Vec::new(), Vec::new());
                for (k, en) in &t[0] {
                    let en = en.to_lowercase();
                    let is_bad = en.contains("could not") || en.contains("failed") || en.contains("in use");
                    let is_good = !is_bad && (en.contains("started") || en.contains("installed"));
                    if !(is_bad || is_good) || !k.starts_with("log_") {
                        continue;
                    }
                    let local = m.get(k).copied().unwrap_or(t[0][k]);
                    let text = local.trim().trim_end_matches([':', '.', '…', ' ', '：', '。']).to_lowercase();
                    if text.chars().count() >= 4 {
                        if is_bad { bad.push(text) } else { good.push(text) }
                    }
                }
                (bad, good)
            })
            .collect()
    });
    &all[lang.0.min(all.len() - 1)]
}

/// Give egui a font for writing systems its own fonts don't cover (CJK, Devanagari), from the Windows fonts folder.
/// Latin and Cyrillic are covered by egui's own fonts; Vietnamese letters with two marks (ạ, ữ, ể...) are not, so
/// Segoe UI (or Arial) is always added last as a fallback — it also draws "Tiếng Việt" in the language list.
pub fn install_fonts(ctx: &egui::Context, lang: Lang) {
    let candidates: &[&str] = match LOCALES[lang.0].3 {
        Script::Latin => &[],
        Script::Japanese => &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"],
        Script::Korean => &["malgun.ttf", "gulim.ttc"],
        Script::ChineseSimplified => &["msyh.ttc", "simsun.ttc"],
        Script::ChineseTraditional => &["msjh.ttc", "mingliu.ttc"],
        Script::Devanagari => &["Nirmala.ttc", "Nirmala.ttf", "mangal.ttf"],
    };
    let mut defs = egui::FontDefinitions::default();
    let fonts = std::path::Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("Fonts");
    if let Some(bytes) = candidates.iter().find_map(|f| std::fs::read(fonts.join(f)).ok()) {
        defs.font_data.insert("system-script".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(family).or_default().push("system-script".into());
        }
    }
    if let Some(bytes) = ["segoeui.ttf", "arial.ttf", "tahoma.ttf"].iter().find_map(|f| std::fs::read(fonts.join(f)).ok()) {
        defs.font_data.insert("system-latin".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(family).or_default().push("system-latin".into());
        }
    }
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language has valid JSON and no key English doesn't have (a typo would never show).
    #[test]
    fn locales_are_valid_and_known() {
        let en: HashMap<String, String> = serde_json::from_str(LOCALES[0].2).unwrap();
        for (code, _, json, _) in LOCALES {
            let m: HashMap<String, String> = serde_json::from_str(json).unwrap_or_else(|e| panic!("{code}: {e}"));
            for k in m.keys() {
                assert!(en.contains_key(k), "{code} has unknown key {k}");
            }
        }
    }

    #[test]
    fn codes_fall_back() {
        assert_eq!(Lang::from_code("pt-PT").code(), "pt-BR");
        assert_eq!(Lang::from_code("es-MX").code(), "es");
        assert_eq!(Lang::from_code("zh-HK").code(), "zh-TW");
        assert_eq!(Lang::from_code("zh-SG").code(), "zh-CN");
        assert_eq!(Lang::from_code("xx-YY").code(), "en");
        let old: Lang = serde_json::from_str("\"Pt\"").unwrap();
        assert_eq!(old.code(), "pt-BR");
    }

    #[test]
    fn missing_text_falls_back_to_english() {
        assert_eq!(tr(Lang::from_code("en"), "start_all"), "Start all");
        assert_ne!(tr(Lang::from_code("pt-BR"), "start_all"), "?");
    }
}
