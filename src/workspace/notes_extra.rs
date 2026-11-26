use super::{Result, length, valid_timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Number;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub title: String,
    pub text: String,
    pub updated_at: Number,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNote {
    pub key: String,
    pub event_id: Uuid,
    pub title: String,
    pub starts_at: i64,
    pub body: String,
    pub updated_at: i64,
}

#[derive(Deserialize)]
pub(super) struct LegacyProductivity {
    pub version: u32,
    pub cards: Vec<Card>,
}

pub(super) fn validate(cards: &[Card], meetings: &[MeetingNote]) -> Result<()> {
    let mut card_ids = HashSet::new();
    if cards.len() > 500
        || cards.iter().map(|card| length(&card.text)).sum::<usize>() > 2_000_000
        || cards.iter().any(|card| {
            card.id.is_empty()
                || length(&card.id) > 100
                || !card_ids.insert(card.id.as_str())
                || card.title.trim().is_empty()
                || length(&card.title) > 240
                || length(&card.text) > 100_000
                || !card.updated_at.as_f64().is_some_and(|value| value >= 0.0)
        })
    {
        return Err("卡片内容或数量超过工作区限制。".into());
    }
    let mut keys = HashSet::new();
    if meetings.len() > 200
        || meetings
            .iter()
            .map(|note| length(&note.body))
            .sum::<usize>()
            > 400_000
        || meetings.iter().any(|note| {
            note.key.is_empty()
                || length(&note.key) > 2400
                || !keys.insert(note.key.as_str())
                || length(&note.title) > 240
                || length(&note.body) > 20_000
                || !valid_timestamp(note.starts_at)
                || !valid_timestamp(note.updated_at)
        })
    {
        return Err("会议笔记内容或数量超过工作区限制。".into());
    }
    Ok(())
}
