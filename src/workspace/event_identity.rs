use super::Event;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) fn identity(event: &Event) -> Uuid {
    let Some(source) = &event.source else {
        return event.id;
    };
    // Subscriptions stored by previous releases already contain the original UID digest.
    let prefix = event
        .subscription_id
        .map(|id| format!("subscription:{id}:"));
    let stored = prefix
        .as_deref()
        .and_then(|p| source.uid.strip_prefix(p))
        .filter(|v| v.len() == 64 && v.bytes().all(|c| c.is_ascii_hexdigit()));
    let uid = stored
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:x}", Sha256::digest(source.uid.as_bytes())));
    let mut hash = Sha256::new();
    hash.update(uid.as_bytes());
    hash.update(source.occurrence.as_bytes());
    Uuid::from_bytes(
        hash.finalize()[..16]
            .try_into()
            .expect("digest is 32 bytes"),
    )
}

pub(crate) fn visible(events: &[Event]) -> Vec<&Event> {
    let mut by_id = HashMap::<Uuid, usize>::new();
    let mut result: Vec<&Event> = Vec::new();
    for event in events {
        let key = identity(event);
        if let Some(&index) = by_id.get(&key) {
            // Prefer the editable invitation, then choose the same subscription across refreshes.
            if (event.subscription_id, event.id) < (result[index].subscription_id, result[index].id)
            {
                result[index] = event;
            }
        } else {
            by_id.insert(key, result.len());
            result.push(event);
        }
    }
    result
}
