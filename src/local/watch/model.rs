use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::super::history::HistoryLiveEvent;
use super::super::observability::{ApiAgent, ApiInvocationDetail};
use super::super::presentation::sanitize_display_text;
use super::super::{PresentationDocument, PresentationKind, PresentationRecord};
use super::client::WatchBootstrap;
use super::input::WatchInput;

const MAX_LIVE_OUTPUT_CHARS: usize = 32 * 1024;
const MAX_LIVE_CARDS: usize = 2_000;
const TRANSIENT_NOTICE_DURATION: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchOptions {
    pub agent: Option<String>,
    pub all: bool,
}

impl WatchOptions {
    pub fn automatic() -> Self {
        Self {
            agent: None,
            all: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WatchScope {
    Waiting,
    Picker,
    Agent(String),
    All,
    Unattributed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ConnectionState {
    Connecting,
    Connected,
    Degraded(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CardKey {
    Invocation(i64),
    Poll {
        target_session_handle: String,
        caller_agent_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct WatchCard {
    key: CardKey,
    pub record: PresentationRecord,
}

#[derive(Debug, Clone, Default)]
struct LiveOutput {
    text: String,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WatchEffect {
    Quit,
    Resubscribe(Option<String>),
    RefreshAgents,
    CycleAgents(isize),
    Copy(String),
}

#[derive(Debug)]
pub(super) struct WatchApp {
    pub runtime_id: String,
    recovery_since_ms: i64,
    pub expires_at: Option<String>,
    pub connection: ConnectionState,
    pub agents: Vec<ApiAgent>,
    pub scope: WatchScope,
    pub picker_index: usize,
    pub selected: usize,
    pub scroll: u16,
    pub search_input: Option<String>,
    pub search_query: String,
    pub status_active_process_count: u64,
    new_workdir_notice: Option<String>,
    new_workdir_notice_expires_at: Option<Instant>,
    recovery_notice: Option<String>,
    recovery_notice_expires_at: Option<Instant>,
    cards: Vec<WatchCard>,
    details: HashMap<i64, ApiInvocationDetail>,
    live_output: HashMap<i64, LiveOutput>,
    expanded: HashSet<CardKey>,
    raw_open: HashSet<i64>,
    automatic_scope: bool,
    last_live_sequence: u64,
}

impl WatchApp {
    pub(super) fn new(bootstrap: &WatchBootstrap, options: WatchOptions) -> Self {
        let automatic_scope = options.agent.is_none() && !options.all;
        let scope = if options.all {
            WatchScope::All
        } else if let Some(agent) = options.agent {
            WatchScope::Agent(agent)
        } else {
            automatic_scope_for_agents(&bootstrap.agents)
        };
        let picker_index = if matches!(scope, WatchScope::Picker) {
            1
        } else {
            0
        };
        Self {
            runtime_id: bootstrap.discovery.runtime_id.clone(),
            recovery_since_ms: now_ms(),
            expires_at: bootstrap.discovery.expires_at.clone(),
            connection: ConnectionState::Connecting,
            agents: bootstrap.agents.clone(),
            scope,
            picker_index,
            selected: 0,
            scroll: 0,
            search_input: None,
            search_query: String::new(),
            status_active_process_count: bootstrap.status.active_process_count,
            new_workdir_notice: None,
            new_workdir_notice_expires_at: None,
            recovery_notice: None,
            recovery_notice_expires_at: None,
            cards: Vec::new(),
            details: HashMap::new(),
            live_output: HashMap::new(),
            expanded: HashSet::new(),
            raw_open: HashSet::new(),
            automatic_scope,
            last_live_sequence: 0,
        }
    }

    pub(super) fn stream_filter(&self) -> Option<String> {
        match &self.scope {
            WatchScope::Agent(id) => Some(id.clone()),
            WatchScope::Waiting
            | WatchScope::Picker
            | WatchScope::All
            | WatchScope::Unattributed => None,
        }
    }

    pub(super) fn recovery_since_ms(&self) -> i64 {
        self.recovery_since_ms
    }

    pub(super) fn set_connected(&mut self) {
        self.connection = ConnectionState::Connected;
    }

    pub(super) fn set_degraded(&mut self, message: impl Into<String>) {
        self.connection = ConnectionState::Degraded(sanitize_display_text(&message.into()));
    }

    pub(super) fn set_agents(&mut self, mut agents: Vec<ApiAgent>) -> Vec<WatchEffect> {
        let previous_scope = self.scope.clone();
        let previous_filter = self.stream_filter();
        sanitize_agents_for_terminal(&mut agents);
        self.agents = agents;
        if self.automatic_scope && matches!(self.scope, WatchScope::Waiting) {
            self.scope = automatic_scope_for_agents(&self.agents);
        }
        if matches!(previous_scope, WatchScope::Waiting) && matches!(self.scope, WatchScope::Picker)
        {
            self.picker_index = default_picker_index(&self.agents);
        } else {
            self.picker_index = self.picker_index.min(self.agents.len());
        }
        if self.scope != previous_scope {
            self.reset_selection();
        }
        if self.stream_filter() != previous_filter {
            return vec![WatchEffect::Resubscribe(self.stream_filter())];
        }
        Vec::new()
    }

    pub(super) fn should_process_live_event(&mut self, event: &HistoryLiveEvent) -> bool {
        if event.sequence <= self.last_live_sequence {
            return false;
        }
        let in_scope = event.event_type == "gap"
            || match &self.scope {
                WatchScope::Agent(id) => event.agent_id.as_deref() == Some(id.as_str()),
                WatchScope::Waiting
                | WatchScope::Picker
                | WatchScope::All
                | WatchScope::Unattributed => true,
            };
        if in_scope {
            self.last_live_sequence = event.sequence;
        }
        in_scope
    }

    pub(super) fn set_status_active_process_count(&mut self, active_process_count: u64) {
        self.status_active_process_count = active_process_count;
    }

    pub(super) fn note_live_event(&mut self, event: &HistoryLiveEvent) -> Vec<WatchEffect> {
        if event.event_type == "gap" {
            let skipped = event
                .payload
                .get("skipped_events")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            self.set_degraded(format!(
                "live event gap ({skipped} skipped); recovering from durable history"
            ));
            return vec![WatchEffect::RefreshAgents];
        }
        if self.automatic_scope
            && matches!(self.scope, WatchScope::Waiting)
            && event.agent_id.is_none()
            && event.invocation_id.is_some()
        {
            self.scope = WatchScope::Unattributed;
            self.reset_selection();
        }
        if event.event_type == "agent_first_seen" {
            return vec![WatchEffect::RefreshAgents];
        }
        if event.event_type == "agent_workdir_added" {
            if let Some(path) = event
                .payload
                .get("normalized_workdir")
                .and_then(serde_json::Value::as_str)
            {
                self.new_workdir_notice = Some(sanitize_display_text(path));
                self.new_workdir_notice_expires_at =
                    Some(Instant::now() + TRANSIENT_NOTICE_DURATION);
            }
            return vec![WatchEffect::RefreshAgents];
        }
        if matches!(
            event.event_type.as_str(),
            "process_started" | "process_ended"
        ) {
            if let Some(count) = event
                .payload
                .get("active_process_count")
                .and_then(serde_json::Value::as_u64)
            {
                self.status_active_process_count = count;
            }
            return vec![WatchEffect::RefreshAgents];
        }
        Vec::new()
    }

    pub(super) fn knows_invocation(&self, id: i64) -> bool {
        self.details.contains_key(&id)
    }

    pub(super) fn known_invocation_ids(&self) -> Vec<i64> {
        self.details.keys().copied().collect()
    }

    pub(super) fn merge_detail(&mut self, detail: ApiInvocationDetail) {
        let invocation_id = detail.invocation.id;
        for mut record in detail.presentation.records.iter().cloned() {
            sanitize_record_for_terminal(&mut record);
            self.merge_record(record);
        }
        self.live_output.remove(&invocation_id);
        self.details.insert(invocation_id, detail);
        self.clamp_selection();
    }

    pub(super) fn merge_presentation(&mut self, presentation: PresentationDocument) {
        for mut record in presentation.records {
            sanitize_record_for_terminal(&mut record);
            if let Some(invocation_id) = record.raw_invocation_ids.first() {
                self.live_output.remove(invocation_id);
            }
            self.merge_record(record);
        }
        self.clamp_selection();
    }

    pub(super) fn append_live_output(&mut self, invocation_id: i64, text: &str) {
        let output = self.live_output.entry(invocation_id).or_default();
        output.text.push_str(&sanitize_display_text(text));
        let count = output.text.chars().count();
        if count > MAX_LIVE_OUTPUT_CHARS {
            let drop_chars = count - MAX_LIVE_OUTPUT_CHARS;
            let byte_index = output
                .text
                .char_indices()
                .nth(drop_chars)
                .map(|(index, _)| index)
                .unwrap_or(0);
            output.text.drain(..byte_index);
            output.truncated = true;
        }
    }

    pub(super) fn visible_cards(&self) -> Vec<&WatchCard> {
        let query = self.search_query.to_ascii_lowercase();
        self.cards
            .iter()
            .filter(|card| self.card_in_scope(card))
            .filter(|card| {
                query.is_empty()
                    || serde_json::to_string(&card.record)
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .collect()
    }

    pub(super) fn selected_card(&self) -> Option<&WatchCard> {
        self.visible_cards().get(self.selected).copied()
    }

    pub(super) fn is_expanded(&self, card: &WatchCard) -> bool {
        self.expanded.contains(&card.key)
    }

    pub(super) fn raw_is_open(&self, card: &WatchCard) -> bool {
        selected_invocation_id(card).is_some_and(|id| self.raw_open.contains(&id))
    }

    pub(super) fn raw_display_for(&self, card: &WatchCard) -> Option<String> {
        let id = selected_invocation_id(card)?;
        let detail = self.details.get(&id)?;
        let exact = raw_evidence_text(detail);
        Some(sanitize_display_text(&exact))
    }

    pub(super) fn live_output_for(&self, card: &WatchCard) -> Option<(&str, bool)> {
        let id = selected_invocation_id(card)?;
        self.live_output
            .get(&id)
            .map(|output| (output.text.as_str(), output.truncated))
    }

    pub(super) fn current_agent(&self) -> Option<&ApiAgent> {
        let WatchScope::Agent(id) = &self.scope else {
            return None;
        };
        self.agents.iter().find(|agent| &agent.id == id)
    }

    pub(super) fn new_workdir_notice(&self) -> Option<&str> {
        self.new_workdir_notice_at(Instant::now())
    }

    pub(super) fn new_workdir_notice_at(&self, now: Instant) -> Option<&str> {
        let visible = self
            .new_workdir_notice_expires_at
            .is_some_and(|expires_at| now < expires_at);
        visible
            .then_some(())
            .and(self.new_workdir_notice.as_deref())
    }

    pub(super) fn set_recovered(&mut self, message: impl Into<String>) {
        self.connection = ConnectionState::Connected;
        self.recovery_notice = Some(sanitize_display_text(&message.into()));
        self.recovery_notice_expires_at = Some(Instant::now() + TRANSIENT_NOTICE_DURATION);
    }

    pub(super) fn recovery_notice(&self) -> Option<&str> {
        self.recovery_notice_at(Instant::now())
    }

    pub(super) fn recovery_notice_at(&self, now: Instant) -> Option<&str> {
        let visible = self
            .recovery_notice_expires_at
            .is_some_and(|expires_at| now < expires_at);
        visible.then_some(()).and(self.recovery_notice.as_deref())
    }

    pub(super) fn ttl_label(&self, now: OffsetDateTime) -> String {
        let Some(expires_at) = self.expires_at.as_deref() else {
            return "TTL: no expiry".to_owned();
        };
        let Ok(expiry) = OffsetDateTime::parse(expires_at, &Rfc3339) else {
            return format!("expires {}", sanitize_display_text(expires_at));
        };
        let seconds = (expiry - now).whole_seconds();
        if seconds <= 0 {
            return "TTL: expired".to_owned();
        }
        if seconds >= 86_400 {
            format!("TTL: {}d {}h", seconds / 86_400, (seconds % 86_400) / 3_600)
        } else if seconds >= 3_600 {
            format!("TTL: {}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
        } else if seconds >= 60 {
            format!("TTL: {}m {}s", seconds / 60, seconds % 60)
        } else {
            format!("TTL: {seconds}s")
        }
    }

    pub(super) fn apply_input(&mut self, input: WatchInput) -> Vec<WatchEffect> {
        if self.search_input.is_some() {
            return self.apply_search_input(input);
        }
        match input {
            WatchInput::Quit => vec![WatchEffect::Quit],
            WatchInput::Move(delta) if matches!(self.scope, WatchScope::Picker) => {
                self.move_picker(delta);
                Vec::new()
            }
            WatchInput::Move(delta) => {
                if self.detail_mode() {
                    self.scroll = if delta.is_negative() {
                        self.scroll.saturating_sub(delta.unsigned_abs() as u16)
                    } else {
                        self.scroll.saturating_add(delta as u16)
                    };
                } else {
                    self.move_selection(delta);
                }
                Vec::new()
            }
            WatchInput::Enter if matches!(self.scope, WatchScope::Picker) => self.choose_picker(),
            WatchInput::Enter => {
                if let Some(key) = self.selected_card().map(|card| card.key.clone())
                    && !self.expanded.remove(&key)
                {
                    self.expanded.insert(key);
                }
                self.scroll = 0;
                Vec::new()
            }
            WatchInput::CycleAgents(direction) => vec![WatchEffect::CycleAgents(direction)],
            WatchInput::OpenPicker => {
                self.picker_index = match &self.scope {
                    WatchScope::Agent(id) => self
                        .agents
                        .iter()
                        .position(|agent| &agent.id == id)
                        .map(|index| index + 1)
                        .unwrap_or_else(|| default_picker_index(&self.agents)),
                    WatchScope::All => 0,
                    _ => default_picker_index(&self.agents),
                };
                self.scope = WatchScope::Picker;
                self.reset_selection();
                vec![WatchEffect::RefreshAgents, WatchEffect::Resubscribe(None)]
            }
            WatchInput::ToggleRaw => {
                if let Some(id) = self.selected_card().and_then(selected_invocation_id)
                    && !self.raw_open.remove(&id)
                {
                    self.raw_open.insert(id);
                }
                self.scroll = 0;
                Vec::new()
            }
            WatchInput::Copy => self
                .selected_card()
                .and_then(|card| self.copy_text_for(card))
                .map(WatchEffect::Copy)
                .into_iter()
                .collect(),
            WatchInput::Top => {
                if self.detail_mode() {
                    self.scroll = 0;
                } else {
                    self.selected = 0;
                    self.scroll = 0;
                }
                Vec::new()
            }
            WatchInput::Bottom => {
                if self.detail_mode() {
                    self.scroll = u16::MAX;
                } else {
                    self.selected = self.visible_cards().len().saturating_sub(1);
                }
                Vec::new()
            }
            WatchInput::StartSearch => {
                self.search_input = Some(self.search_query.clone());
                Vec::new()
            }
            WatchInput::SearchChar(_)
            | WatchInput::SearchBackspace
            | WatchInput::SearchCommit
            | WatchInput::SearchCancel => Vec::new(),
        }
    }

    fn apply_search_input(&mut self, input: WatchInput) -> Vec<WatchEffect> {
        match input {
            WatchInput::SearchChar(ch) => self.search_input.as_mut().unwrap().push(ch),
            WatchInput::SearchBackspace => {
                self.search_input.as_mut().unwrap().pop();
            }
            WatchInput::SearchCommit => {
                self.search_query = self.search_input.take().unwrap_or_default();
                self.reset_selection();
            }
            WatchInput::SearchCancel | WatchInput::Quit => self.search_input = None,
            _ => {}
        }
        Vec::new()
    }

    fn merge_record(&mut self, record: PresentationRecord) {
        let key = card_key(&record);
        self.cards.retain(|card| {
            card.key == key
                || card
                    .record
                    .raw_invocation_ids
                    .iter()
                    .all(|id| !record.raw_invocation_ids.contains(id))
        });
        if matches!(key, CardKey::Poll { .. })
            && let Some(index) = self.cards.iter().position(|card| card.key == key)
        {
            merge_poll_record(&mut self.cards[index].record, &record);
            self.sort_cards();
            return;
        }
        if let Some(existing) = self.cards.iter_mut().find(|card| card.key == key) {
            existing.record = record;
            return;
        }
        self.cards.push(WatchCard { key, record });
        self.sort_cards();
        if self.cards.len() > MAX_LIVE_CARDS {
            self.cards.remove(0);
        }
    }

    fn sort_cards(&mut self) {
        self.cards.sort_by_key(|card| {
            (
                card.record.started_at_ms,
                card.record.raw_invocation_ids.first().copied().unwrap_or(0),
            )
        });
    }

    fn card_in_scope(&self, card: &WatchCard) -> bool {
        match &self.scope {
            WatchScope::Agent(id) => card.record.agent_id.as_deref() == Some(id),
            WatchScope::Unattributed => card.record.agent_id.is_none(),
            WatchScope::All => true,
            WatchScope::Waiting | WatchScope::Picker => false,
        }
    }

    fn move_picker(&mut self, delta: isize) {
        self.picker_index = move_index(self.picker_index, self.agents.len() + 1, delta);
    }

    fn choose_picker(&mut self) -> Vec<WatchEffect> {
        self.scope = if self.picker_index == 0 {
            WatchScope::All
        } else {
            WatchScope::Agent(self.agents[self.picker_index - 1].id.clone())
        };
        self.reset_selection();
        vec![WatchEffect::Resubscribe(self.stream_filter())]
    }

    pub(super) fn cycle_agents_after_refresh(&mut self, direction: isize) -> Vec<WatchEffect> {
        if self.agents.is_empty() {
            return Vec::new();
        }
        let current = match &self.scope {
            WatchScope::Agent(id) => self.agents.iter().position(|agent| &agent.id == id),
            _ => None,
        };
        let next = match (current, direction.is_negative()) {
            (Some(index), false) => (index + 1) % self.agents.len(),
            (Some(0), true) => self.agents.len() - 1,
            (Some(index), true) => index - 1,
            (None, true) => self.agents.len() - 1,
            (None, false) => 0,
        };
        self.scope = WatchScope::Agent(self.agents[next].id.clone());
        self.reset_selection();
        vec![WatchEffect::Resubscribe(self.stream_filter())]
    }

    fn move_selection(&mut self, delta: isize) {
        self.selected = move_index(self.selected, self.visible_cards().len(), delta);
    }

    fn detail_mode(&self) -> bool {
        self.selected_card()
            .is_some_and(|card| self.is_expanded(card) || self.raw_is_open(card))
    }

    fn copy_text_for(&self, card: &WatchCard) -> Option<String> {
        let id = selected_invocation_id(card)?;
        let detail = self.details.get(&id)?;
        if self.raw_open.contains(&id) {
            return Some(raw_evidence_text(detail));
        }
        let arguments = &detail.invocation.arguments;
        match detail.invocation.tool_name.as_str() {
            "exec_command" => arguments
                .get("cmd")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            "apply_patch" => arguments
                .get("patch")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            "write_stdin" => arguments
                .get("chars")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(raw_evidence_text(detail))),
            _ => Some(raw_evidence_text(detail)),
        }
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    fn clamp_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.visible_cards().len().saturating_sub(1));
    }
}

fn automatic_scope_for_agents(agents: &[ApiAgent]) -> WatchScope {
    match agents {
        [] => WatchScope::Waiting,
        [agent] => WatchScope::Agent(agent.id.clone()),
        _ => WatchScope::Picker,
    }
}

fn default_picker_index(agents: &[ApiAgent]) -> usize {
    usize::from(!agents.is_empty())
}

fn now_ms() -> i64 {
    let now = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    i64::try_from(now).unwrap_or_else(|_| {
        if now.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

fn card_key(record: &PresentationRecord) -> CardKey {
    if let PresentationKind::PollAggregate {
        target_session_handle,
        ..
    } = &record.kind
    {
        CardKey::Poll {
            target_session_handle: target_session_handle.clone(),
            caller_agent_id: record.agent_id.clone(),
        }
    } else {
        CardKey::Invocation(record.raw_invocation_ids.first().copied().unwrap_or(0))
    }
}

fn selected_invocation_id(card: &WatchCard) -> Option<i64> {
    match &card.key {
        CardKey::Invocation(id) => (*id > 0).then_some(*id),
        CardKey::Poll { .. } => card.record.raw_invocation_ids.last().copied(),
    }
}

fn sanitize_agents_for_terminal(agents: &mut [ApiAgent]) {
    for agent in agents {
        for workdir in &mut agent.workdirs {
            workdir.normalized_workdir = sanitize_display_text(&workdir.normalized_workdir);
        }
    }
}

fn sanitize_record_for_terminal(record: &mut PresentationRecord) {
    record.agent_id = record
        .agent_id
        .take()
        .map(|value| sanitize_display_text(&value));
    record.declared_workdir = record
        .declared_workdir
        .take()
        .map(|value| sanitize_display_text(&value));
    record.normalized_workdir = record
        .normalized_workdir
        .take()
        .map(|value| sanitize_display_text(&value));
    record.new_workdir = record
        .new_workdir
        .take()
        .map(|value| sanitize_display_text(&value));
    record.evidence.evidence_state = sanitize_display_text(&record.evidence.evidence_state);
    record.evidence.capture_state = sanitize_display_text(&record.evidence.capture_state);
    record.evidence.reason = record
        .evidence
        .reason
        .take()
        .map(|value| sanitize_display_text(&value));

    match &mut record.kind {
        PresentationKind::Command {
            command,
            status,
            effective_cwd,
            termination_reason,
            output,
            polls,
            ..
        } => {
            *command = sanitize_display_text(command);
            *status = sanitize_display_text(status);
            *effective_cwd = effective_cwd
                .take()
                .map(|value| sanitize_display_text(&value));
            *termination_reason = termination_reason
                .take()
                .map(|value| sanitize_display_text(&value));
            *output = output.take().map(|value| sanitize_display_text(&value));
            if let Some(polls) = polls {
                polls.final_status = polls
                    .final_status
                    .take()
                    .map(|value| sanitize_display_text(&value));
                for caller in &mut polls.caller_agent_ids {
                    *caller = sanitize_display_text(caller);
                }
            }
        }
        PresentationKind::FileChanges {
            source_tool,
            changes,
        } => {
            *source_tool = sanitize_display_text(source_tool);
            for change in changes {
                change.path = sanitize_display_text(&change.path);
                change.old_path = change
                    .old_path
                    .take()
                    .map(|value| sanitize_display_text(&value));
                for line in &mut change.lines {
                    line.kind = sanitize_display_text(&line.kind);
                    line.text = sanitize_display_text(&line.text);
                }
            }
        }
        PresentationKind::Stdin {
            target_session_handle,
            chars,
            creator_agent_id,
            result_status,
            ..
        } => {
            *target_session_handle = sanitize_display_text(target_session_handle);
            *chars = sanitize_display_text(chars);
            *creator_agent_id = creator_agent_id
                .take()
                .map(|value| sanitize_display_text(&value));
            *result_status = result_status
                .take()
                .map(|value| sanitize_display_text(&value));
        }
        PresentationKind::Kill {
            target_session_handle,
            creator_agent_id,
            result_status,
            ..
        } => {
            *target_session_handle = sanitize_display_text(target_session_handle);
            *creator_agent_id = creator_agent_id
                .take()
                .map(|value| sanitize_display_text(&value));
            *result_status = result_status
                .take()
                .map(|value| sanitize_display_text(&value));
        }
        PresentationKind::PollAggregate {
            target_session_handle,
            final_status,
            creator_agent_id,
            caller_agent_ids,
            ..
        } => {
            *target_session_handle = sanitize_display_text(target_session_handle);
            *final_status = final_status
                .take()
                .map(|value| sanitize_display_text(&value));
            *creator_agent_id = creator_agent_id
                .take()
                .map(|value| sanitize_display_text(&value));
            for caller in caller_agent_ids {
                *caller = sanitize_display_text(caller);
            }
        }
        PresentationKind::Generic {
            tool_name,
            status,
            summary,
        } => {
            *tool_name = sanitize_display_text(tool_name);
            *status = sanitize_display_text(status);
            *summary = summary.take().map(|value| sanitize_display_text(&value));
        }
    }
}

fn merge_poll_record(existing: &mut PresentationRecord, incoming: &PresentationRecord) {
    let existing_latest = (
        existing.started_at_ms,
        existing
            .raw_invocation_ids
            .iter()
            .copied()
            .max()
            .unwrap_or(0),
    );
    let incoming_latest = (
        incoming.started_at_ms,
        incoming
            .raw_invocation_ids
            .iter()
            .copied()
            .max()
            .unwrap_or(0),
    );
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
    for id in incoming.raw_invocation_ids.iter().copied() {
        if !existing.raw_invocation_ids.contains(&id) {
            existing.raw_invocation_ids.push(id);
        }
    }
    existing.raw_invocation_ids.sort_unstable();
    *existing_count = existing.raw_invocation_ids.len();
    merge_evidence(&mut existing.evidence, &incoming.evidence);
}

fn merge_evidence(
    existing: &mut super::super::PresentationEvidence,
    incoming: &super::super::PresentationEvidence,
) {
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

fn raw_evidence_text(detail: &ApiInvocationDetail) -> String {
    serde_json::to_string_pretty(&json!({
        "id": detail.invocation.id,
        "tool_name": detail.invocation.tool_name,
        "arguments": detail.invocation.arguments,
        "result": detail.invocation.result,
        "error": detail.invocation.error,
    }))
    .unwrap_or_else(|_| "{\"error\":\"raw evidence unavailable\"}".to_owned())
}

fn move_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize).min(len - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::move_index;

    #[test]
    fn move_index_is_bounded() {
        assert_eq!(move_index(0, 0, 1), 0);
        assert_eq!(move_index(0, 3, -1), 0);
        assert_eq!(move_index(1, 3, 1), 2);
        assert_eq!(move_index(2, 3, 1), 2);
    }
}
