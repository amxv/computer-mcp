use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use tracing::{error, warn};

use crate::invocation::{
    InvocationContext, InvocationEvidenceRecorder, InvocationOutcome, InvocationStart,
};
use crate::local::file_evidence::{
    CompletedFileEvidence, PendingFileEvidence, complete_file_evidence, prepare_file_evidence,
};
use crate::local::presentation::{PRESENTATION_SCHEMA_VERSION, sanitize_display_text};
use crate::session::{
    OwnedProcess, OwnedProcessEnd, OwnedProcessObserver, SessionOutputChunk,
    SessionOutputCompletion, SessionOutputObserver,
};

use super::events::{HistoryEventHub, HistoryLiveEvent};
use super::store::{
    HistoryStore, OutputEvent, distinct_invocation_ids, mark_retention_error_best_effort,
    normalize_declared_workdir, now_ms,
};

mod output_queue;

const DEFAULT_OUTPUT_QUEUE_CAPACITY: usize = 4096;
const OUTPUT_BATCH_LIMIT: usize = 128;
const OUTPUT_BATCH_WAIT: Duration = Duration::from_millis(25);
const RETENTION_MIN_INTERVAL: Duration = Duration::from_secs(30);
// Remote MCP ingress and command-session shutdown are already closed before
// this evidence-only drain runs. Give the detached PTY reader enough room to
// observe EOF even on a heavily loaded host, while retaining a hard bound so
// a descendant that keeps the slave open cannot stall Local shutdown forever.
const SHUTDOWN_CAPTURE_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub struct LocalHistoryRuntimeConfig {
    pub database_path: PathBuf,
    pub runtime_id: Arc<str>,
    pub max_age_seconds: u64,
    pub max_size_bytes: u64,
    pub output_queue_capacity: usize,
    pub event_capacity: usize,
}

impl LocalHistoryRuntimeConfig {
    pub fn new(
        database_path: PathBuf,
        runtime_id: impl Into<Arc<str>>,
        max_age_seconds: u64,
        max_size_bytes: u64,
    ) -> Self {
        Self {
            database_path,
            runtime_id: runtime_id.into(),
            max_age_seconds,
            max_size_bytes,
            output_queue_capacity: DEFAULT_OUTPUT_QUEUE_CAPACITY,
            event_capacity: HistoryEventHub::default_capacity(),
        }
    }

    pub fn with_output_queue_capacity(mut self, capacity: usize) -> Self {
        self.output_queue_capacity = capacity.max(1);
        self
    }

    pub fn with_event_capacity(mut self, capacity: usize) -> Self {
        self.event_capacity = capacity.max(1);
        self
    }
}

struct HistoryHealth {
    accepting_new: AtomicBool,
    reason: Mutex<Option<String>>,
    incomplete_evidence_invocations: Mutex<HashSet<i64>>,
    incomplete_capture_invocations: Mutex<HashSet<i64>>,
    maintenance_requested: AtomicBool,
}

impl HistoryHealth {
    fn new() -> Self {
        Self {
            accepting_new: AtomicBool::new(true),
            reason: Mutex::new(None),
            incomplete_evidence_invocations: Mutex::new(HashSet::new()),
            incomplete_capture_invocations: Mutex::new(HashSet::new()),
            maintenance_requested: AtomicBool::new(false),
        }
    }

    fn ensure_accepting(&self) -> Result<()> {
        if self.accepting_new.load(Ordering::Acquire) {
            return Ok(());
        }
        let reason = self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| "unknown evidence-pipeline failure".to_string());
        bail!("Local history evidence pipeline is degraded: {reason}")
    }

    fn degrade_nonblocking(&self, reason: impl Into<String>) {
        let reason = reason.into();
        self.accepting_new.store(false, Ordering::Release);
        let mut guard = self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(reason.clone());
        }
        drop(guard);
        error!(event = "local_history_degraded", error = %reason);
    }

    fn degrade_persisting(&self, store: &HistoryStore, reason: impl Into<String>) {
        self.degrade_nonblocking(reason);
        self.persist_degraded_state(store);
    }

    fn persist_degraded_state(&self, store: &HistoryStore) {
        if self.accepting_new.load(Ordering::Acquire) {
            return;
        }
        let reason = self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| "unknown evidence-pipeline failure".to_string());
        if let Err(error) = store.set_health("degraded", Some(&reason)) {
            error!(
                event = "local_history_health_persistence_failed",
                original_error = %reason,
                error = %error,
            );
        }
        let incomplete_evidence = self
            .incomplete_evidence_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for invocation_id in incomplete_evidence {
            if let Err(error) = store.mark_evidence_incomplete(invocation_id, &reason) {
                error!(
                    event = "local_history_evidence_incomplete_persistence_failed",
                    invocation_id,
                    error = %error,
                );
            }
        }
        let incomplete_capture = self
            .incomplete_capture_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for invocation_id in incomplete_capture {
            if let Err(error) = store.mark_capture_incomplete(invocation_id, &reason) {
                error!(
                    event = "local_history_capture_incomplete_persistence_failed",
                    invocation_id,
                    error = %error,
                );
            }
        }
    }

    fn note_evidence_incomplete(&self, invocation_id: i64) {
        self.incomplete_evidence_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(invocation_id);
    }

    fn note_capture_incomplete(&self, invocation_id: i64) {
        self.incomplete_capture_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(invocation_id);
    }
}

enum WorkerMessage {
    Output(OutputEvent),
    Complete {
        context: InvocationContext,
        outcome: InvocationOutcome,
        file_evidence: Vec<CompletedFileEvidence>,
    },
    #[cfg(test)]
    Barrier(std::sync::mpsc::Sender<()>),
    Shutdown,
}

pub struct LocalHistoryRuntime {
    store: Arc<HistoryStore>,
    runtime_id: Arc<str>,
    events: Arc<HistoryEventHub>,
    sender: SyncSender<WorkerMessage>,
    output_overflow: Mutex<output_queue::OutputOverflow>,
    worker: Mutex<Option<JoinHandle<()>>>,
    health: Arc<HistoryHealth>,
    file_evidence: Mutex<HashMap<i64, Vec<PendingFileEvidence>>>,
    active_process_invocation_ids: Mutex<HashSet<i64>>,
    active_process_counts: Mutex<HashMap<String, u64>>,
    active_process_count: AtomicU64,
}

impl LocalHistoryRuntime {
    pub fn open(config: LocalHistoryRuntimeConfig) -> Result<Arc<Self>> {
        Self::open_with_store(config, None)
    }

    pub(super) fn open_with_store(
        config: LocalHistoryRuntimeConfig,
        store_override: Option<Arc<HistoryStore>>,
    ) -> Result<Arc<Self>> {
        let runtime_id = config.runtime_id.clone();
        let events = HistoryEventHub::new(runtime_id.clone(), config.event_capacity);
        let store = match store_override {
            Some(store) => store,
            None => Arc::new(HistoryStore::open(
                config.database_path,
                runtime_id.clone(),
            )?),
        };
        let max_age_seconds = config.max_age_seconds;
        let max_size_bytes = config.max_size_bytes;
        // Enforce retention before the runtime is returned and can admit its
        // first invocation. Keeping this startup pass out of the writer loop
        // prevents maintenance from cutting in front of the first queued
        // completion while preserving immediate startup cleanup.
        if let Err(error) = store.run_retention(max_age_seconds, max_size_bytes) {
            mark_retention_error_best_effort(&store, &error.to_string());
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(config.output_queue_capacity.max(1));
        let health = Arc::new(HistoryHealth::new());
        let worker_store = store.clone();
        let worker_health = health.clone();
        let worker_events = events.clone();
        let worker = std::thread::Builder::new()
            .name("zodex-local-history".to_string())
            .spawn(move || {
                run_worker(
                    receiver,
                    worker_store,
                    worker_health,
                    worker_events,
                    max_age_seconds,
                    max_size_bytes,
                )
            })?;
        Ok(Arc::new(Self {
            store,
            runtime_id,
            events,
            sender,
            output_overflow: Mutex::new(output_queue::OutputOverflow::default()),
            worker: Mutex::new(Some(worker)),
            health,
            file_evidence: Mutex::new(HashMap::new()),
            active_process_invocation_ids: Mutex::new(HashSet::new()),
            active_process_counts: Mutex::new(HashMap::new()),
            active_process_count: AtomicU64::new(0),
        }))
    }

    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub fn active_process_counts(&self) -> HashMap<String, u64> {
        self.active_process_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn active_process_count(&self) -> u64 {
        self.active_process_count.load(Ordering::Acquire)
    }

    pub(crate) fn active_process_invocation_ids(&self) -> Vec<i64> {
        self.active_process_invocation_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .copied()
            .collect()
    }

    pub(crate) fn subscribe_live_events(
        &self,
    ) -> (u64, tokio::sync::broadcast::Receiver<HistoryLiveEvent>) {
        (self.events.current_sequence(), self.events.subscribe())
    }

    #[cfg(test)]
    pub(crate) fn live_event_subscriber_count(&self) -> usize {
        self.events.receiver_count()
    }

    #[cfg(test)]
    pub(crate) fn live_event_sequence(&self) -> u64 {
        self.events.current_sequence()
    }

    pub fn database_path(&self) -> &std::path::Path {
        self.store.path()
    }

    pub fn accepting_new_invocations(&self) -> bool {
        self.health.accepting_new.load(Ordering::Acquire)
    }

    pub fn physical_size_bytes(&self) -> Result<u64> {
        self.store.physical_size()
    }

    pub fn request_retention(&self) {
        self.health
            .maintenance_requested
            .store(true, Ordering::Release);
    }

    pub fn run_retention_now(&self, max_age_seconds: u64, max_size_bytes: u64) -> Result<()> {
        self.store.run_retention(max_age_seconds, max_size_bytes)
    }

    #[cfg(test)]
    pub(crate) fn flush_for_test(&self) -> Result<()> {
        let (acknowledge, acknowledged) = std::sync::mpsc::channel();
        self.sender
            .send(WorkerMessage::Barrier(acknowledge))
            .map_err(|_| {
                anyhow::anyhow!("Local history writer is unavailable during test flush")
            })?;
        acknowledged.recv().map_err(|_| {
            anyhow::anyhow!("Local history writer stopped before test flush completed")
        })
    }

    pub fn shutdown_blocking(&self) -> Result<()> {
        let handle = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(handle) = handle else {
            return Ok(());
        };
        // The command-result path intentionally has a short PTY drain bound so
        // a descendant that keeps the slave open cannot hold the MCP response
        // forever. Runtime shutdown owns a separate bounded evidence-finalize
        // window: keep the history worker alive while any late reader EOF/EIO
        // markers arrive, then mark the remaining captures explicitly
        // incomplete rather than leaving durable `pending` rows behind.
        let capture_deadline = Instant::now() + SHUTDOWN_CAPTURE_DRAIN_TIMEOUT;
        let mut finalize_error = None;
        loop {
            let pending = match self.store.pending_capture_ids() {
                Ok(pending) => pending,
                Err(error) => {
                    warn!(
                        event = "local_history_shutdown_capture_query_failed",
                        error = %error,
                    );
                    finalize_error = Some(
                        error.context("failed while finalizing Local output capture at shutdown"),
                    );
                    break;
                }
            };
            if pending.is_empty() {
                break;
            }
            if Instant::now() >= capture_deadline {
                for invocation_id in pending {
                    if let Err(error) = self.store.mark_capture_incomplete(
                        invocation_id,
                        "Local runtime shutdown ended before PTY capture reached terminal EOF",
                    ) {
                        warn!(
                            event = "local_history_shutdown_capture_finalize_failed",
                            invocation_id,
                            error = %error,
                        );
                        if finalize_error.is_none() {
                            finalize_error = Some(error.context(
                                "failed to finalize incomplete Local output capture at shutdown",
                            ));
                        }
                    }
                }
                break;
            }
            std::thread::sleep(SHUTDOWN_CAPTURE_POLL_INTERVAL);
        }
        // After the finalize window, a blocking send is safe: the worker keeps
        // draining everything already queued before it observes Shutdown. A
        // pathological late producer may still race after the deadline, but
        // that invocation is already durably marked capture-incomplete.
        let _ = self.sender.send(WorkerMessage::Shutdown);
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("Local history writer thread panicked"))?;
        if let Err(error) = self.store.mark_pending_file_evidence_unavailable(
            "Local runtime shutdown ended before after-file evidence completed",
        ) {
            warn!(
                event = "local_history_shutdown_file_evidence_finalize_failed",
                error = %error,
            );
            if finalize_error.is_none() {
                finalize_error = Some(error);
            }
        }
        if let Some(error) = finalize_error {
            Err(error)
        } else {
            Ok(())
        }
    }
}

impl InvocationEvidenceRecorder for LocalHistoryRuntime {
    fn begin(
        &self,
        context: InvocationContext,
        start: InvocationStart,
    ) -> Result<InvocationContext> {
        self.health.ensure_accepting()?;
        let pending_file_evidence = prepare_file_evidence(&start);
        let tool_name = start.tool_name.clone();
        let normalized_workdir = start
            .arguments
            .get("workdir")
            .and_then(serde_json::Value::as_str)
            .and_then(normalize_declared_workdir);
        match self.store.begin_with_metadata(context, start) {
            Ok(begin) => {
                let context = begin.context;
                let invocation_id = context.invocation_id;
                let agent_id = context.agent_id.as_deref();
                if begin.agent_first_seen_in_runtime {
                    self.events.emit_with(
                        "agent_first_seen",
                        agent_id,
                        invocation_id,
                        None,
                        || json!({}),
                    );
                }
                if let Some(new_workdir) = begin.new_workdir.as_deref() {
                    self.events.emit_with(
                        "agent_workdir_added",
                        agent_id,
                        invocation_id,
                        Some(PRESENTATION_SCHEMA_VERSION),
                        || json!({"normalized_workdir": new_workdir}),
                    );
                }
                self.events.emit_with(
                    "invocation_started",
                    agent_id,
                    invocation_id,
                    Some(PRESENTATION_SCHEMA_VERSION),
                    || {
                        json!({
                            "tool_name": tool_name.as_ref(),
                            "normalized_workdir": normalized_workdir,
                        })
                    },
                );
                if let Some(invocation_id) = context.invocation_id
                    && !pending_file_evidence.is_empty()
                {
                    match self
                        .store
                        .persist_file_evidence_start(invocation_id, &pending_file_evidence)
                    {
                        Ok(()) => {
                            self.file_evidence
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .insert(invocation_id, pending_file_evidence);
                        }
                        Err(error) => warn!(
                            event = "local_file_evidence_before_persistence_failed",
                            invocation_id,
                            error = %error,
                        ),
                    }
                }
                Ok(context)
            }
            Err(error) => {
                self.health.degrade_persisting(
                    &self.store,
                    format!("invocation envelope persistence failed: {error}"),
                );
                Err(error)
            }
        }
    }

    fn complete(&self, context: &InvocationContext, outcome: InvocationOutcome) -> Result<()> {
        let invocation_id = context.invocation_id.ok_or_else(|| {
            anyhow::anyhow!("Local invocation completion is missing durable invocation ID")
        })?;
        let pending_file_evidence = self
            .file_evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&invocation_id)
            .unwrap_or_default();
        let file_evidence = complete_file_evidence(&pending_file_evidence);
        let queued = WorkerMessage::Complete {
            context: context.clone(),
            outcome: outcome.clone(),
            file_evidence: file_evidence.clone(),
        };
        let reason = match self.sender.try_send(queued) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(_)) => "Local history completion queue is full".to_string(),
            Err(TrySendError::Disconnected(_)) => {
                "Local history completion writer is unavailable".to_string()
            }
        };

        // A saturated output queue must never make us discard the exact
        // handler result. Degrade future admission immediately, then make one
        // bounded direct SQLite attempt for this already-admitted invocation.
        // This exceptional fallback may wait for SQLite's short busy timeout;
        // normal completions remain asynchronous and never block the response
        // path on the history writer.
        self.health.degrade_nonblocking(&reason);
        match self.store.complete(context, outcome.clone()) {
            Ok(()) => {
                persist_completed_file_evidence(&self.store, invocation_id, &file_evidence);
                emit_completion_events(&self.events, context, &outcome);
                self.health
                    .maintenance_requested
                    .store(true, Ordering::Release);
                self.health.persist_degraded_state(&self.store);
                Ok(())
            }
            Err(error) => {
                self.health.note_evidence_incomplete(invocation_id);
                self.health.persist_degraded_state(&self.store);
                Err(anyhow::anyhow!(
                    "{reason}; direct exact-completion persistence also failed: {error}"
                ))
            }
        }
    }
}

impl SessionOutputObserver for LocalHistoryRuntime {
    fn observe_output(&self, chunk: SessionOutputChunk) {
        let Some(invocation_id) = chunk.invocation.invocation_id else {
            return;
        };
        let observed_at_ms = match now_ms() {
            Ok(value) => value,
            Err(error) => {
                let reason = format!("failed to timestamp Local PTY output: {error}");
                self.health.note_capture_incomplete(invocation_id);
                self.health.degrade_nonblocking(reason);
                return;
            }
        };
        self.try_enqueue_output_chunk(
            OutputEvent::Chunk {
                invocation_id,
                agent_id: chunk.invocation.agent_id.as_deref().map(str::to_owned),
                sequence: chunk.sequence,
                observed_at_ms,
                text: chunk.text,
            },
            invocation_id,
        );
    }

    fn observe_output_complete(&self, completion: SessionOutputCompletion) {
        let Some(invocation_id) = completion.invocation.invocation_id else {
            return;
        };
        self.enqueue_output_completion(
            WorkerMessage::Output(OutputEvent::Complete {
                invocation_id,
                agent_id: completion.invocation.agent_id.as_deref().map(str::to_owned),
            }),
            invocation_id,
        );
    }
}

impl OwnedProcessObserver for LocalHistoryRuntime {
    fn process_started(&self, process: &OwnedProcess) -> Result<()> {
        let invocation_id = process
            .created_by
            .invocation_id
            .context("active Local process is missing durable creator invocation ID")?;
        if let Err(error) = self.store.process_started(invocation_id) {
            self.health.degrade_persisting(
                &self.store,
                format!("process lifecycle start persistence failed: {error}"),
            );
            return Err(error);
        }
        if let Err(error) = self.store.protect_active_process_invocation(invocation_id) {
            self.health.degrade_persisting(
                &self.store,
                format!("active process retention protection failed: {error}"),
            );
            return Err(error);
        }
        self.active_process_invocation_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(invocation_id);
        let active_process_count = self.active_process_count.fetch_add(1, Ordering::AcqRel) + 1;
        let agent_id = process.created_by.agent_id.as_deref();
        let agent_active_process_count = agent_id.map(|agent_id| {
            let mut counts = self
                .active_process_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = counts.entry(agent_id.to_string()).or_default();
            *count = count.saturating_add(1);
            *count
        });
        self.events.emit_with(
            "process_started",
            agent_id,
            Some(invocation_id),
            None,
            || {
                json!({
                    "active_process_count": active_process_count,
                    "agent_active_process_count": agent_active_process_count,
                })
            },
        );
        Ok(())
    }

    fn process_ended(&self, process: &OwnedProcess, end: &OwnedProcessEnd) -> Result<()> {
        let Some(invocation_id) = process.created_by.invocation_id else {
            return Ok(());
        };
        let lifecycle_result = self.store.process_ended(invocation_id, end);
        if let Err(error) = &lifecycle_result {
            self.health.degrade_persisting(
                &self.store,
                format!("process lifecycle end persistence failed: {error}"),
            );
        }
        let retention_result = self
            .store
            .unprotect_active_process_invocation(invocation_id);
        let was_active = self
            .active_process_invocation_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&invocation_id);
        let active_process_count = if was_active {
            self.active_process_count
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    Some(count.saturating_sub(1))
                })
                .unwrap_or(0)
                .saturating_sub(1)
        } else {
            self.active_process_count.load(Ordering::Acquire)
        };
        let agent_id = process.created_by.agent_id.as_deref();
        let agent_active_process_count = agent_id.map(|agent_id| {
            let mut counts = self
                .active_process_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = counts.entry(agent_id.to_string()).or_default();
            if was_active {
                *count = count.saturating_sub(1);
            }
            let active = *count;
            if active == 0 {
                counts.remove(agent_id);
            }
            active
        });
        if lifecycle_result.is_ok() {
            self.events
                .emit_with("process_ended", agent_id, Some(invocation_id), None, || {
                    json!({
                        "active_process_count": active_process_count,
                        "agent_active_process_count": agent_active_process_count,
                    })
                });
        }
        lifecycle_result?;
        retention_result
    }
}

fn run_worker(
    receiver: Receiver<WorkerMessage>,
    store: Arc<HistoryStore>,
    health: Arc<HistoryHealth>,
    events: Arc<HistoryEventHub>,
    max_age_seconds: u64,
    max_size_bytes: u64,
) {
    let mut shutdown = false;
    // `open_with_store` completed the startup retention pass before spawning
    // this worker, so future maintenance can honor the normal coalescing
    // interval instead of racing the first evidence messages.
    let mut last_maintenance = Some(Instant::now());
    while !shutdown {
        let mut messages = Vec::with_capacity(OUTPUT_BATCH_LIMIT);
        match receiver.recv_timeout(OUTPUT_BATCH_WAIT) {
            Ok(message @ (WorkerMessage::Output(_) | WorkerMessage::Complete { .. })) => {
                messages.push(message)
            }
            #[cfg(test)]
            Ok(message @ WorkerMessage::Barrier(_)) => messages.push(message),
            Ok(WorkerMessage::Shutdown) => shutdown = true,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while messages.len() < OUTPUT_BATCH_LIMIT {
            match receiver.try_recv() {
                Ok(message @ (WorkerMessage::Output(_) | WorkerMessage::Complete { .. })) => {
                    messages.push(message)
                }
                #[cfg(test)]
                Ok(message @ WorkerMessage::Barrier(_)) => messages.push(message),
                Ok(WorkerMessage::Shutdown) => {
                    shutdown = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shutdown = true;
                    break;
                }
            }
        }

        process_messages(messages, &store, &health, &events);

        let maintenance_due = health.maintenance_requested.load(Ordering::Acquire)
            && last_maintenance
                .map(|last| last.elapsed() >= RETENTION_MIN_INTERVAL)
                .unwrap_or(true);
        if maintenance_due && health.maintenance_requested.swap(false, Ordering::AcqRel) {
            last_maintenance = Some(Instant::now());
            if let Err(error) = store.run_retention(max_age_seconds, max_size_bytes) {
                mark_retention_error_best_effort(&store, &error.to_string());
            }
        }
    }

    // One final maintenance pass after all queued output has been flushed.
    if let Err(error) = store.run_retention(max_age_seconds, max_size_bytes) {
        warn!(event = "local_history_shutdown_retention_failed", error = %error);
    }
    health.persist_degraded_state(&store);
}

fn process_messages(
    messages: Vec<WorkerMessage>,
    store: &HistoryStore,
    health: &HistoryHealth,
    events: &HistoryEventHub,
) {
    let mut output_events = Vec::new();
    for message in messages {
        match message {
            WorkerMessage::Output(event) => output_events.push(event),
            WorkerMessage::Complete {
                context,
                outcome,
                file_evidence,
            } => {
                flush_output_events(&mut output_events, store, health, events);
                let invocation_id = context.invocation_id;
                if let Err(error) = store.complete(&context, outcome.clone()) {
                    if let Some(invocation_id) = invocation_id {
                        health.note_evidence_incomplete(invocation_id);
                    }
                    health.degrade_persisting(
                        store,
                        format!("invocation completion persistence failed: {error}"),
                    );
                } else {
                    if let Some(invocation_id) = invocation_id {
                        persist_completed_file_evidence(store, invocation_id, &file_evidence);
                    }
                    emit_completion_events(events, &context, &outcome);
                    health.maintenance_requested.store(true, Ordering::Release);
                }
            }
            #[cfg(test)]
            WorkerMessage::Barrier(acknowledge) => {
                flush_output_events(&mut output_events, store, health, events);
                let _ = acknowledge.send(());
            }
            WorkerMessage::Shutdown => {
                unreachable!("shutdown messages are consumed by the worker loop")
            }
        }
    }
    flush_output_events(&mut output_events, store, health, events);
    if !health.accepting_new.load(Ordering::Acquire) {
        health.persist_degraded_state(store);
    }
}

fn persist_completed_file_evidence(
    store: &HistoryStore,
    invocation_id: i64,
    evidence: &[CompletedFileEvidence],
) {
    if evidence.is_empty() {
        return;
    }
    if let Err(error) = store.persist_file_evidence_completion(invocation_id, evidence) {
        warn!(
            event = "local_file_evidence_after_persistence_failed",
            invocation_id,
            error = %error,
        );
        let reason = format!("after-file evidence persistence failed: {error}");
        if let Err(mark_error) = store.mark_file_evidence_unavailable(invocation_id, &reason) {
            warn!(
                event = "local_file_evidence_unavailable_persistence_failed",
                invocation_id,
                error = %mark_error,
            );
        }
    }
}

fn flush_output_events(
    events: &mut Vec<OutputEvent>,
    store: &HistoryStore,
    health: &HistoryHealth,
    live_events: &HistoryEventHub,
) {
    if events.is_empty() {
        return;
    }
    if let Err(error) = store.persist_output_batch(events) {
        let reason = format!("output evidence batch persistence failed: {error}");
        for invocation_id in distinct_invocation_ids(events) {
            health.note_capture_incomplete(invocation_id);
        }
        health.degrade_persisting(store, reason);
    } else {
        for event in events.iter() {
            match event {
                OutputEvent::Chunk {
                    invocation_id,
                    agent_id,
                    sequence,
                    text,
                    ..
                } => live_events.emit_with(
                    "output",
                    agent_id.as_deref(),
                    Some(*invocation_id),
                    None,
                    || {
                        json!({
                            "output_sequence": sequence,
                            "text": sanitize_display_text(text),
                        })
                    },
                ),
                OutputEvent::Complete {
                    invocation_id,
                    agent_id,
                } => live_events.emit_with(
                    "output_complete",
                    agent_id.as_deref(),
                    Some(*invocation_id),
                    None,
                    || json!({}),
                ),
            }
        }
    }
    events.clear();
}

fn emit_completion_events(
    events: &HistoryEventHub,
    context: &InvocationContext,
    outcome: &InvocationOutcome,
) {
    let invocation_id = context.invocation_id;
    let agent_id = context.agent_id.as_deref();
    let outcome_kind = match outcome {
        InvocationOutcome::Success(_) => "success",
        InvocationOutcome::Error(_) => "error",
    };
    events.emit_with(
        "invocation_completed",
        agent_id,
        invocation_id,
        Some(PRESENTATION_SCHEMA_VERSION),
        || json!({"outcome": outcome_kind}),
    );
    events.emit_with(
        "presentation_updated",
        agent_id,
        invocation_id,
        Some(PRESENTATION_SCHEMA_VERSION),
        || json!({}),
    );
}
