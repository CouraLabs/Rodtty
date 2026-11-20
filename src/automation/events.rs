//! Ordered events, jobs and retry-safe mutations for automation clients.
//!
//! The desktop instance keeps one [`Hub`]. Events have increasing sequence
//! numbers in a bounded history; a reader whose cursor fell out of it is told
//! so and resynchronizes from a state snapshot. Jobs track long-running
//! operations by ID. Request IDs make mutations retry-safe: a repeated request
//! returns the first result instead of acting again.
use super::{ErrorCode, ToolError, ToolResult};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const EVENT_HISTORY: usize = 2048;
pub const JOB_HISTORY: usize = 256;
pub const REQUEST_HISTORY: usize = 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub seq: u64,
    pub at_ms: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub data: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    WaitingForOwner,
}

impl JobState {
    pub fn finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub client: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub state: JobState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolError>,
    pub created_ms: u64,
    pub updated_ms: u64,
    #[serde(skip)]
    pub cancel: Arc<AtomicBool>,
}

enum Request {
    InFlight,
    Done(ToolResult),
}

struct Remembered {
    key: String,
    fingerprint: String,
    state: Request,
}

#[derive(Default)]
struct Inner {
    events: VecDeque<Event>,
    next_seq: u64,
    jobs: VecDeque<Job>,
    requests: VecDeque<Remembered>,
}

/// What a request ID says about a mutation that is about to run.
pub enum Begin {
    /// First time: run it, then call [`Hub::finish`].
    New,
    /// Already ran: this is its result.
    Done(ToolResult),
}

pub struct Hub {
    inner: Mutex<Inner>,
    changed: Condvar,
    started: Instant,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            changed: Condvar::new(),
            started: Instant::now(),
        }
    }
}

impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn emit(&self, kind: &str, target: Option<&str>, data: Value) -> u64 {
        let at_ms = self.now_ms();
        let mut inner = self.lock();
        inner.next_seq += 1;
        let seq = inner.next_seq;
        if inner.events.len() == EVENT_HISTORY {
            inner.events.pop_front();
        }
        inner.events.push_back(Event {
            seq,
            at_ms,
            kind: kind.to_owned(),
            target: target.map(str::to_owned),
            data,
        });
        drop(inner);
        self.changed.notify_all();
        seq
    }

    /// The cursor a new reader starts from: after every event so far.
    pub fn cursor(&self) -> u64 {
        self.lock().next_seq
    }

    /// Events after `cursor`, waiting up to `wait` for the first one. When
    /// `cursor` is older than the retained history, `expired` is true and
    /// the reader should take a fresh snapshot.
    pub fn read(&self, cursor: u64, max: usize, wait: Duration, cancel: &AtomicBool) -> Value {
        let deadline = Instant::now() + wait;
        let mut inner = self.lock();
        loop {
            if inner.next_seq > cursor
                || Instant::now() >= deadline
                || cancel.load(Ordering::Relaxed)
            {
                break;
            }
            let left = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100));
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        let oldest = inner.events.front().map_or(inner.next_seq + 1, |e| e.seq);
        let expired = cursor + 1 < oldest && cursor < inner.next_seq;
        let events: Vec<&Event> = inner
            .events
            .iter()
            .filter(|e| e.seq > cursor)
            .take(max)
            .collect();
        let next = events.last().map_or(
            cursor.max(if expired { inner.next_seq } else { cursor }),
            |e| e.seq,
        );
        json!({
            "events": events,
            "next_cursor": next,
            "expired": expired,
            "oldest_retained": oldest,
            "latest": inner.next_seq,
        })
    }

    pub fn start_job(
        &self,
        kind: &str,
        client: &str,
        target: Option<&str>,
    ) -> (String, Arc<AtomicBool>) {
        let id = format!("job_{}", super::token());
        let cancel = Arc::new(AtomicBool::new(false));
        let now = self.now_ms();
        let mut inner = self.lock();
        if inner.jobs.len() >= JOB_HISTORY
            && let Some(index) = inner.jobs.iter().position(|j| j.state.finished())
        {
            inner.jobs.remove(index);
        }
        inner.jobs.push_back(Job {
            id: id.clone(),
            kind: kind.to_owned(),
            client: client.to_owned(),
            target: target.map(str::to_owned),
            state: JobState::Running,
            result: None,
            error: None,
            created_ms: now,
            updated_ms: now,
            cancel: cancel.clone(),
        });
        drop(inner);
        self.emit(
            "job",
            Some(&id),
            json!({"state": "running", "kind": kind, "terminal": target}),
        );
        (id, cancel)
    }

    pub fn update_job(
        &self,
        id: &str,
        state: JobState,
        result: Option<Value>,
        error: Option<ToolError>,
    ) {
        let now = self.now_ms();
        let mut inner = self.lock();
        let Some(job) = inner.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        if job.state.finished() {
            return;
        }
        job.state = state;
        job.updated_ms = now;
        if result.is_some() {
            job.result = result;
        }
        if error.is_some() {
            job.error = error;
        }
        let summary = json!({"state": state, "kind": job.kind});
        drop(inner);
        self.emit("job", Some(id), summary);
    }

    pub fn job(&self, id: &str) -> Option<Job> {
        self.lock().jobs.iter().find(|j| j.id == id).cloned()
    }

    pub fn jobs_for(&self, client: &str) -> Vec<Job> {
        self.lock()
            .jobs
            .iter()
            .filter(|j| j.client == client)
            .cloned()
            .collect()
    }

    /// Ask a job to stop. Only that operation stops: cancelling a wait
    /// leaves its terminal application running.
    pub fn cancel_job(&self, id: &str) -> bool {
        let job = self.lock().jobs.iter().find(|j| j.id == id).cloned();
        let Some(job) = job else {
            return false;
        };
        job.cancel.store(true, Ordering::Relaxed);
        if !job.state.finished() {
            self.update_job(id, JobState::Cancelled, None, None);
        }
        true
    }

    /// Cancel every unfinished job of `client`, as a pause or revocation does.
    pub fn cancel_client(&self, client: &str) -> usize {
        let ids: Vec<String> = self
            .lock()
            .jobs
            .iter()
            .filter(|j| j.client == client && !j.state.finished())
            .map(|j| j.id.clone())
            .collect();
        for id in &ids {
            self.cancel_job(id);
        }
        ids.len()
    }

    /// Wait until job `id` finishes or `wait` passes.
    pub fn wait_job(&self, id: &str, wait: Duration, cancel: &AtomicBool) -> Option<Job> {
        let deadline = Instant::now() + wait;
        let mut inner = self.lock();
        loop {
            let job = inner.jobs.iter().find(|j| j.id == id)?.clone();
            if job.state.finished() || Instant::now() >= deadline || cancel.load(Ordering::Relaxed)
            {
                return Some(job);
            }
            let left = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100));
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Register request `key` (client, tool and request ID) before running
    /// it. A retry of a request still running waits for its result.
    pub fn begin(&self, key: &str, fingerprint: &str) -> Result<Begin, ToolError> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut inner = self.lock();
        loop {
            match inner.requests.iter().find(|r| r.key == key) {
                None => break,
                Some(r) if r.fingerprint != fingerprint => {
                    return Err(ToolError::new(
                        ErrorCode::Conflict,
                        "This request_id was already used with different arguments",
                    ));
                }
                Some(Remembered {
                    state: Request::Done(result),
                    ..
                }) => {
                    return Ok(Begin::Done(result.clone()));
                }
                Some(_) if Instant::now() >= deadline => {
                    return Err(ToolError::new(
                        ErrorCode::Timeout,
                        "The first attempt of this request is still running",
                    ));
                }
                Some(_) => {
                    inner = self
                        .changed
                        .wait_timeout(inner, Duration::from_millis(100))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
        if inner.requests.len() == REQUEST_HISTORY {
            inner.requests.pop_front();
        }
        inner.requests.push_back(Remembered {
            key: key.to_owned(),
            fingerprint: fingerprint.to_owned(),
            state: Request::InFlight,
        });
        Ok(Begin::New)
    }

    pub fn finish(&self, key: &str, result: &ToolResult) {
        let mut inner = self.lock();
        if let Some(r) = inner.requests.iter_mut().find(|r| r.key == key) {
            r.state = Request::Done(result.clone());
        }
        drop(inner);
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_see_ordered_events_and_learn_when_history_expired() {
        let hub = Hub::default();
        let never = AtomicBool::new(false);
        let start = hub.cursor();
        hub.emit("a", None, json!(1));
        hub.emit("b", Some("t"), json!(2));
        let read = hub.read(start, 10, Duration::ZERO, &never);
        let kinds: Vec<_> = read["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].clone())
            .collect();
        assert_eq!(kinds, [json!("a"), json!("b")]);
        assert_eq!(read["expired"], false);
        for _ in 0..EVENT_HISTORY + 5 {
            hub.emit("x", None, json!(null));
        }
        let read = hub.read(start, 10, Duration::ZERO, &never);
        assert_eq!(read["expired"], true);
    }

    #[test]
    fn a_repeated_request_returns_its_first_result() {
        let hub = Hub::default();
        assert!(matches!(hub.begin("c/launch/1", "f").unwrap(), Begin::New));
        hub.finish("c/launch/1", &Ok(json!({"terminal": "t1"})));
        match hub.begin("c/launch/1", "f").unwrap() {
            Begin::Done(Ok(value)) => assert_eq!(value["terminal"], "t1"),
            _ => panic!("expected the remembered result"),
        }
        assert_eq!(
            hub.begin("c/launch/1", "other").err().unwrap().code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn cancelling_a_job_stops_only_that_job() {
        let hub = Hub::default();
        let (a, flag_a) = hub.start_job("wait", "c", None);
        let (b, flag_b) = hub.start_job("wait", "c", None);
        assert!(hub.cancel_job(&a));
        assert!(flag_a.load(Ordering::Relaxed));
        assert!(!flag_b.load(Ordering::Relaxed));
        assert_eq!(hub.job(&a).unwrap().state, JobState::Cancelled);
        assert_eq!(hub.job(&b).unwrap().state, JobState::Running);
        assert_eq!(hub.cancel_client("c"), 1);
    }
}
