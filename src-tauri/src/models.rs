//! Serializable settings, OCR frames, and personal vocabulary.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkey: String,
    pub gamepad: String,
    pub auto_pause: bool,
    pub dialogue_only: bool,
    pub overlay: bool,
    pub capture_backend: String,
    pub translator: TranslatorSettings,
    pub decision: crate::decision::DecisionSettings,
    pub subtitles: crate::subtitles::SubtitleSettings,
}
impl Default for Settings {
    /// Returns a conflict-resistant default binding and local OCR preferences.
    fn default() -> Self {
        Self {
            hotkey: "F8".into(),
            gamepad: "shoulders".into(),
            auto_pause: true,
            dialogue_only: true,
            overlay: true,
            capture_backend: "auto".into(),
            translator: TranslatorSettings::default(),
            decision: crate::decision::DecisionSettings::default(),
            subtitles: crate::subtitles::SubtitleSettings::default(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub word: String,
    pub translation: String,
    pub context: String,
    pub game: String,
    pub created_at: u64,
    #[serde(default)]
    pub analysis: Option<ContextTranslation>,
    #[serde(default)]
    pub review_count: u32,
    #[serde(default)]
    pub learned: bool,
    #[serde(default)]
    pub query_count: u64,
    #[serde(default)]
    pub due_at: u64,
    #[serde(default)]
    pub interval_days: u32,
    #[serde(default)]
    pub streak: u32,
    #[serde(default)]
    pub lapses: u32,
    #[serde(default)]
    pub last_reviewed: u64,
}
#[derive(Default, Serialize, Deserialize)]
pub struct SavedData {
    pub settings: Settings,
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub lookups: Vec<LookupStat>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Game {
    pub pid: u32,
    pub name: String,
    pub title: String,
    pub path: String,
    pub hwnd: usize,
    pub api: String,
    pub x64: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub line: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Ocr {
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub words: Vec<Word>,
}
#[derive(Clone, Serialize)]
pub struct Frame {
    pub image: String,
    pub ocr: Ocr,
    pub paused: bool,
    pub warning: Option<String>,
    pub history: Vec<String>,
    pub game_title: String,
    pub blocks: Vec<crate::context::Block>,
}
#[derive(Serialize)]
pub struct Status {
    pub game: Option<Game>,
    pub paused: bool,
    pub busy: bool,
    pub settings: Settings,
    pub entries: Vec<Entry>,
    pub dictionary_size: usize,
    pub capture_mode: String,
}
#[derive(Serialize)]
pub struct Definition {
    pub query: String,
    pub lemma: String,
    pub translations: Vec<String>,
    pub source: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TranslatorSettings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub target_language: String,
    pub history_lines: usize,
    pub mini_model: String,
    pub threads: u32,
    pub device: String,
    pub preload: bool,
    pub auto_install: bool,
}
impl Default for TranslatorSettings {
    /// Keeps network translation opt-in until a local model or cloud API is configured.
    fn default() -> Self {
        Self {
            provider: "mini".into(),
            endpoint: "http://127.0.0.1:11434".into(),
            model: String::new(),
            target_language: "ru".into(),
            history_lines: 6,
            mini_model: "opus".into(),
            threads: 0,
            device: "cpu".into(),
            preload: true,
            auto_install: true,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ContextTranslation {
    pub selection: String,
    pub translation: String,
    pub context_translation: String,
    pub phrase: String,
    pub explanation: String,
    pub construction: String,
    pub alternatives: Vec<String>,
    pub source: String,
    pub idiom: bool,
    #[serde(default)]
    pub grammar: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub tokens: Vec<GrammarToken>,
    #[serde(default)]
    pub situation: String,
    #[serde(default)]
    pub grammar_source: String,
    #[serde(default)]
    pub query_count: u64,
    #[serde(default)]
    pub decisions: Option<crate::decision::Evidence>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct GrammarToken {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub lemma: String,
    pub pos: String,
    pub label: String,
    pub role: String,
    pub verb_form: String,
    pub irregular: bool,
    pub forms: Vec<String>,
    pub dependency: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct GrammarAnalysis {
    pub tokens: Vec<GrammarToken>,
    pub source: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct LookupStat {
    pub phrase: String,
    pub count: u64,
    pub last_seen: u64,
    pub contexts: Vec<String>,
    pub game: String,
    pub patterns: Vec<String>,
    pub analysis: ContextTranslation,
}
