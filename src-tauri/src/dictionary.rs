//! Offline FreeDict lookup, conservative inflections, and durable personal entries.
use crate::models::{Definition, SavedData};
use std::{collections::HashMap, fs, path::Path};
pub type Dictionary = HashMap<String, Vec<String>>;

/// Normalizes an OCR token while preserving internal apostrophes and hyphens.
pub fn normalize(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphabetic())
        .replace('’', "'")
        .to_lowercase()
}
/// Tries literal and common English inflections; reports the actual matched lemma.
pub fn lookup(dictionary: &Dictionary, saved: &SavedData, query: &str) -> Definition {
    let query = normalize(query);
    if let Some(entry) = saved.entries.iter().find(|e| normalize(&e.word) == query) {
        return Definition {
            query: query.clone(),
            lemma: query,
            translations: vec![entry.translation.clone()],
            source: "Личный словарь".into(),
        };
    }
    let mut candidates = vec![query.clone()];
    let irregular = [
        ("was", "be"),
        ("were", "be"),
        ("been", "be"),
        ("went", "go"),
        ("gone", "go"),
        ("saw", "see"),
        ("seen", "see"),
        ("had", "have"),
        ("did", "do"),
        ("done", "do"),
        ("said", "say"),
        ("thought", "think"),
        ("found", "find"),
        ("took", "take"),
        ("taken", "take"),
        ("brought", "bring"),
        ("knew", "know"),
        ("known", "know"),
        ("made", "make"),
        ("children", "child"),
        ("people", "person"),
    ];
    if let Some((_, lemma)) = irregular.iter().find(|(word, _)| *word == query) {
        candidates.push((*lemma).into());
    }
    if let Some(stem) = query.strip_suffix("'s") {
        candidates.push(stem.into());
    }
    if let Some(stem) = query.strip_suffix("ies") {
        candidates.push(format!("{stem}y"));
    }
    for suffix in ["s", "es", "ed", "ing", "ly"] {
        if let Some(stem) = query.strip_suffix(suffix).filter(|s| s.len() > 2) {
            candidates.push(stem.into());
            if suffix == "ing" || suffix == "ed" {
                candidates.push(format!("{stem}e"));
                let chars: Vec<char> = stem.chars().collect();
                if chars.len() > 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
                    candidates.push(chars[..chars.len() - 1].iter().collect());
                }
                if let Some(stem) = stem.strip_suffix('i') {
                    candidates.push(format!("{stem}y"));
                }
            }
        }
    }
    for lemma in candidates {
        if let Some(translations) = dictionary.get(&lemma) {
            return Definition {
                query,
                lemma,
                translations: translations.clone(),
                source: "FreeDict · EN → RU · офлайн".into(),
            };
        }
    }
    Definition {
        query: query.clone(),
        lemma: query,
        translations: vec![],
        source: "Нет статьи · можно добавить свой перевод".into(),
    }
}
/// Writes the whole store through a temporary file before replacing the old copy.
/// Returns an error if serialization or either filesystem operation fails.
pub fn persist(path: &Path, data: &SavedData) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())
}
