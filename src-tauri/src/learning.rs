//! Persistent query counters and examples, independent of flashcard review counts.
use crate::{dictionary, models::*, runtime::Runtime};
use std::time::{SystemTime, UNIX_EPOCH};

/// Schedules a contextual flashcard using a bounded, explicit Leitner-style sequence.
pub fn schedule(entry: &mut Entry, remembered: bool) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    entry.last_reviewed = now;
    if remembered {
        const DAYS: [u32; 8] = [1, 3, 7, 14, 30, 60, 120, 240];
        entry.interval_days = DAYS[(entry.streak as usize).min(DAYS.len() - 1)];
        entry.streak = entry.streak.saturating_add(1);
        entry.due_at = now.saturating_add(entry.interval_days as u64 * 86400);
    } else {
        entry.streak = 0;
        entry.interval_days = 0;
        entry.lapses = entry.lapses.saturating_add(1);
        entry.due_at = now.saturating_add(600);
    }
}

/// Groups selected words inside a known idiom under the whole idiom.
fn key(analysis: &ContextTranslation) -> String {
    dictionary::normalize(if analysis.idiom {
        &analysis.phrase
    } else {
        &analysis.selection
    })
}
/// Counts one user selection and keeps at most five distinct recent contexts per phrase.
pub fn record(
    rt: &Runtime,
    analysis: &mut ContextTranslation,
    context: &str,
) -> Result<(), String> {
    let phrase = key(analysis);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut inner = rt.inner.lock().unwrap();
    let previous = serde_json::to_vec(&inner.data).map_err(|e| e.to_string())?;
    let game = inner
        .game
        .as_ref()
        .map(|g| g.title.clone())
        .unwrap_or_default();
    let index = inner
        .data
        .lookups
        .iter()
        .position(|s| s.phrase == phrase)
        .unwrap_or_else(|| {
            inner.data.lookups.push(LookupStat {
                phrase: phrase.clone(),
                count: 0,
                last_seen: now,
                contexts: vec![],
                game: game.clone(),
                patterns: vec![],
                analysis: analysis.clone(),
            });
            inner.data.lookups.len() - 1
        });
    let stat = &mut inner.data.lookups[index];
    stat.count = stat.count.saturating_add(1);
    stat.last_seen = now;
    stat.game = game;
    let context = context.chars().take(3000).collect::<String>();
    if !context.is_empty() && stat.contexts.last() != Some(&context) {
        stat.contexts.push(context);
        if stat.contexts.len() > 5 {
            stat.contexts.remove(0);
        }
    }
    analysis.query_count = stat.count;
    stat.analysis = analysis.clone();
    let count = stat.count;
    if let Some(entry) = inner.data.entries.iter_mut().find(|e| e.word == phrase) {
        entry.query_count = count;
    }
    if let Err(e) = dictionary::persist(&rt.store, &inner.data) {
        inner.data = serde_json::from_slice(&previous).unwrap();
        return Err(e);
    }
    Ok(())
}
/// Attaches completed analysis without counting a second request for the same selection.
pub fn update(rt: &Runtime, analysis: &mut ContextTranslation) -> Result<(), String> {
    let phrase = key(analysis);
    let mut inner = rt.inner.lock().unwrap();
    let previous = serde_json::to_vec(&inner.data).map_err(|e| e.to_string())?;
    if let Some(stat) = inner.data.lookups.iter_mut().find(|s| s.phrase == phrase) {
        analysis.query_count = stat.count;
        stat.analysis = analysis.clone();
        let mut patterns = std::collections::BTreeSet::new();
        if analysis.idiom {
            patterns.insert("Устойчивые выражения".to_owned());
        }
        for token in &analysis.tokens {
            if !token.verb_form.is_empty() {
                patterns.insert(token.verb_form.clone());
            }
            if token.irregular {
                patterns.insert("Неправильные глаголы".into());
            }
            if token.pos == "NOUN" || token.pos == "ADJ" {
                patterns.insert(token.label.clone());
            }
            if token.role == "Подлежащее" || token.role == "Объект" {
                patterns.insert(token.role.clone());
            }
        }
        stat.patterns = patterns.into_iter().collect();
    }
    if let Err(e) = dictionary::persist(&rt.store, &inner.data) {
        inner.data = serde_json::from_slice(&previous).unwrap();
        return Err(e);
    }
    Ok(())
}
#[derive(serde::Serialize)]
pub struct Statistics {
    pub total_queries: u64,
    pub phrases: Vec<LookupStat>,
    pub constructions: Vec<(String, u64)>,
}
/// Returns the most queried phrases and construction counts without bloating the input poll.
pub fn statistics(rt: &Runtime) -> Statistics {
    let inner = rt.inner.lock().unwrap();
    let total_queries = inner.data.lookups.iter().map(|s| s.count).sum();
    let mut groups = std::collections::BTreeMap::<String, u64>::new();
    for stat in &inner.data.lookups {
        for pattern in &stat.patterns {
            *groups.entry(pattern.clone()).or_default() += stat.count;
        }
    }
    let mut phrases = inner.data.lookups.clone();
    phrases.sort_by_key(|s| std::cmp::Reverse(s.count));
    phrases.truncate(100);
    let mut constructions = groups.into_iter().collect::<Vec<_>>();
    constructions.sort_by_key(|s| std::cmp::Reverse(s.1));
    Statistics {
        total_queries,
        phrases,
        constructions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Older vocabulary records should acquire scheduling fields without migration loss.
    #[test]
    fn legacy_card_and_schedule() {
        let mut card: Entry=serde_json::from_str(r#"{"word":"written","translation":"написанный","context":"She has written a letter.","game":"probe","created_at":1}"#).unwrap();
        assert_eq!(card.due_at, 0);
        for days in [1, 3, 7, 14, 30, 60, 120, 240, 240] {
            schedule(&mut card, true);
            assert_eq!(card.interval_days, days);
            assert_eq!(card.due_at - card.last_reviewed, days as u64 * 86400);
        }
        schedule(&mut card, false);
        assert_eq!(card.due_at - card.last_reviewed, 600);
        assert_eq!(card.streak, 0);
        assert_eq!(card.lapses, 1);
        schedule(&mut card, true);
        assert_eq!(card.interval_days, 1);
        let restored: Entry = serde_json::from_str(&serde_json::to_string(&card).unwrap()).unwrap();
        assert_eq!(restored.due_at, card.due_at);
        assert_eq!(restored.context, card.context);
    }
}
