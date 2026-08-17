use super::super::presentation::PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT;
use super::super::{PresentationEvidence, PresentationKind, PresentationRecord};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CardKey(String);

#[derive(Debug, Clone)]
pub(super) struct WatchCard {
    pub(super) key: CardKey,
    pub record: PresentationRecord,
}

impl CardKey {
    pub(super) fn from_record(record: &PresentationRecord) -> Self {
        Self(record.presentation_id.clone())
    }
}

pub(super) fn selected_invocation_id(card: &WatchCard) -> Option<i64> {
    (card.record.primary_invocation_id > 0).then_some(card.record.primary_invocation_id)
}

pub(super) fn merge_card(
    cards: &mut Vec<WatchCard>,
    record: PresentationRecord,
    duplicate_detail: bool,
) {
    let key = CardKey::from_record(&record);
    if let Some(index) = cards.iter().position(|card| card.key == key) {
        if duplicate_detail
            && matches!(
                cards[index].record.kind,
                PresentationKind::PollAggregate { .. }
            )
            && matches!(record.kind, PresentationKind::PollAggregate { .. })
        {
            merge_legacy_poll_record(&mut cards[index].record, &record, true);
            return;
        }
        cards[index].record = record;
        let authoritative_ids = cards[index].record.raw_invocation_ids.clone();
        cards.retain(|card| {
            card.key == key
                || !card
                    .record
                    .raw_invocation_ids
                    .iter()
                    .any(|id| authoritative_ids.contains(id))
        });
        return;
    }

    if let Some(index) = legacy_poll_merge_index(cards, &record) {
        let incoming_has_retained_parent = !record
            .raw_invocation_ids
            .contains(&record.primary_invocation_id);
        if incoming_has_retained_parent || record.raw_evidence_count > 1 {
            cards[index] = WatchCard { key, record };
        } else {
            merge_legacy_poll_record(&mut cards[index].record, &record, duplicate_detail);
        }
        return;
    }

    // Stable presentation identity is authoritative. Raw overlap is retained
    // only as a compatibility cleanup for a legacy orphan poll card that is
    // later replaced by its canonical parent command after reconciliation.
    cards.retain(|card| !raw_samples_overlap(&card.record, &record));
    cards.push(WatchCard { key, record });
}

fn legacy_poll_merge_index(cards: &[WatchCard], incoming: &PresentationRecord) -> Option<usize> {
    let PresentationKind::PollAggregate {
        target_session_handle,
        ..
    } = &incoming.kind
    else {
        return None;
    };
    cards.iter().position(|card| {
        card.record.agent_id == incoming.agent_id
            && matches!(
                &card.record.kind,
                PresentationKind::PollAggregate {
                    target_session_handle: existing_handle,
                    ..
                } if existing_handle == target_session_handle
            )
    })
}

fn raw_samples_overlap(left: &PresentationRecord, right: &PresentationRecord) -> bool {
    left.raw_invocation_ids
        .iter()
        .any(|id| right.raw_invocation_ids.contains(id))
}

fn merge_legacy_poll_record(
    existing: &mut PresentationRecord,
    incoming: &PresentationRecord,
    duplicate_detail: bool,
) {
    let existing_latest = (existing.started_at_ms, latest_sample_id(existing));
    let incoming_latest = (incoming.started_at_ms, latest_sample_id(incoming));
    let PresentationKind::PollAggregate {
        count: existing_count,
        final_status,
        creator_agent_id,
        caller_agent_ids,
        cross_agent,
        ..
    } = &mut existing.kind
    else {
        return;
    };
    let PresentationKind::PollAggregate {
        final_status: incoming_status,
        creator_agent_id: incoming_creator,
        caller_agent_ids: incoming_callers,
        cross_agent: incoming_cross_agent,
        ..
    } = &incoming.kind
    else {
        return;
    };

    if incoming_latest > existing_latest {
        *final_status = incoming_status.clone();
        existing.started_at_ms = incoming.started_at_ms;
        existing.duration_ms = incoming.duration_ms;
    }
    if creator_agent_id.is_none() {
        *creator_agent_id = incoming_creator.clone();
    }
    for caller in incoming_callers {
        if !caller_agent_ids.contains(caller) {
            caller_agent_ids.push(caller.clone());
        }
    }
    *cross_agent |= *incoming_cross_agent;

    if !duplicate_detail {
        existing.raw_evidence_count = existing
            .raw_evidence_count
            .saturating_add(incoming.raw_evidence_count);
        *existing_count = existing.raw_evidence_count;
        for id in incoming.raw_invocation_ids.iter().copied() {
            if existing.raw_invocation_ids.len() >= PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT {
                break;
            }
            if !existing.raw_invocation_ids.contains(&id) {
                existing.raw_invocation_ids.push(id);
            }
        }
        existing.raw_invocation_ids.sort_unstable();
        existing.raw_invocation_ids_truncated =
            existing.raw_evidence_count > existing.raw_invocation_ids.len();
    }
    merge_evidence(&mut existing.evidence, &incoming.evidence);
}

fn latest_sample_id(record: &PresentationRecord) -> i64 {
    record.raw_invocation_ids.iter().copied().max().unwrap_or(0)
}

fn merge_evidence(existing: &mut PresentationEvidence, incoming: &PresentationEvidence) {
    existing.evidence_state =
        merge_evidence_state(&existing.evidence_state, &incoming.evidence_state);
    existing.capture_state = merge_capture_state(&existing.capture_state, &incoming.capture_state);
    existing.degraded |= incoming.degraded;
    if existing.reason.is_none() {
        existing.reason = incoming.reason.clone();
    }
}

fn merge_evidence_state(existing: &str, incoming: &str) -> String {
    if existing == "incomplete" || incoming == "incomplete" {
        "incomplete"
    } else if existing == "pending" || incoming == "pending" {
        "pending"
    } else {
        "complete"
    }
    .to_owned()
}

fn merge_capture_state(existing: &str, incoming: &str) -> String {
    if existing == "incomplete" || incoming == "incomplete" {
        "incomplete"
    } else if existing == "pending" || incoming == "pending" {
        "pending"
    } else if existing == "complete" || incoming == "complete" {
        "complete"
    } else {
        "not_applicable"
    }
    .to_owned()
}
