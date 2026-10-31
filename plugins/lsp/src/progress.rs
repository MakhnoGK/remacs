//! What a server is busy with, for the status: starting, loading the project, or the work
//! it reports with `$/progress`.

use std::time::{Duration, Instant};

use remacs_core::StatusItem;
use serde_json::Value;

/// A piece of work a server reported, by its token.
#[derive(Debug, PartialEq)]
struct Work {
    token: Value,
    title: String,
    counter: Option<String>,
    percent: Option<u8>,
}

#[derive(Debug)]
pub(crate) struct Progress {
    started: Instant,
    /// Since when the server has been busy, while it is.
    busy_since: Option<Instant>,
    initialized: bool,
    /// False while the server says it is loading the project.
    settled: bool,
    /// In the order it began.
    work: Vec<Work>,
    /// How long the server took to be first idle, until taken.
    ready_after: Option<Duration>,
    was_ready: bool,
}

impl Progress {
    pub(crate) fn new(started: Instant, settled: bool) -> Self {
        Self {
            started,
            busy_since: Some(started),
            initialized: false,
            settled,
            work: Vec::new(),
            ready_after: None,
            was_ready: false,
        }
    }

    /// False while the server says it is loading the project.
    pub(crate) fn is_settled(&self) -> bool {
        self.settled
    }

    /// Whether `change` changed what the status shows, so the editor need wake only then.
    fn changes(&mut self, now: Instant, change: impl FnOnce(&mut Self)) -> bool {
        let before = self.status("");
        change(self);
        self.refresh(now);
        self.status("") != before
    }

    pub(crate) fn initialized(&mut self, now: Instant) -> bool {
        self.changes(now, |progress| progress.initialized = true)
    }

    /// True when it flipped too: requests waiting for the server to settle are asked again.
    pub(crate) fn settle(&mut self, settled: bool, now: Instant) -> bool {
        let flipped = self.settled != settled;
        self.changes(now, |progress| progress.settled = settled) || flipped
    }

    /// Takes in a `$/progress` notification's params.
    pub(crate) fn report(&mut self, params: &Value, now: Instant) -> bool {
        self.changes(now, |progress| progress.take_report(params))
    }

    fn take_report(&mut self, params: &Value) {
        let (token, value) = (&params["token"], &params["value"]);
        // A message without a counter takes away the one before; none at all keeps it.
        let message_counter = value["message"].as_str().map(counter);
        let percent = value["percentage"].as_u64().map(|percent| percent.min(100) as u8);
        match value["kind"].as_str() {
            Some("begin") => self.work.push(Work {
                token: token.clone(),
                title: value["title"].as_str().unwrap_or_default().to_owned(),
                counter: message_counter.flatten(),
                percent,
            }),
            Some("report") => {
                if let Some(work) = self.work.iter_mut().find(|work| work.token == *token) {
                    work.counter = message_counter.unwrap_or(work.counter.take());
                    work.percent = percent.or(work.percent);
                }
            }
            Some("end") => self.work.retain(|work| work.token != *token),
            _ => {}
        }
    }

    fn refresh(&mut self, now: Instant) {
        let busy = !self.initialized || !self.settled || !self.work.is_empty();
        match (busy, self.busy_since) {
            (true, None) => self.busy_since = Some(now),
            (false, Some(_)) => {
                self.busy_since = None;
                if !std::mem::replace(&mut self.was_ready, true) {
                    self.ready_after = Some(now - self.started);
                }
            }
            _ => {}
        }
    }

    /// How long the server took to start and load, once it first has.
    pub(crate) fn take_ready(&mut self) -> Option<Duration> {
        self.ready_after.take()
    }

    /// What server `name` is busy with, if it is.
    pub(crate) fn status(&self, name: &str) -> Option<StatusItem> {
        let since = self.busy_since?;
        let Some(work) = self.work.first().filter(|_| self.initialized) else {
            let doing = if self.initialized { "loading" } else { "starting" };
            return Some(StatusItem::working(format!("{name} {doing}…"), since, None));
        };
        let text = match (work.percent, &work.counter) {
            (Some(percent), _) => format!("{name}: {} {percent}%", work.title),
            (None, Some(counter)) => format!("{name}: {} {counter}", work.title),
            (None, None) => format!("{name}: {}", work.title),
        };
        Some(StatusItem::working(text, since, work.percent))
    }
}

/// The `536/635` a message starts with, if it does; the rest, like the paths
/// rust-analyzer names, changes too fast and too wide for the status.
fn counter(message: &str) -> Option<String> {
    let first = message.split_whitespace().next()?.trim_end_matches(':');
    let (done, total) = first.split_once('/')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    (digits(done) && digits(total)).then(|| first.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn progress_params(token: &str, value: Value) -> Value {
        json!({ "token": token, "value": value })
    }

    #[test]
    fn a_server_is_starting_until_it_answers_initialize() {
        let start = Instant::now();
        let mut progress = Progress::new(start, true);
        assert_eq!(progress.status("ra"), Some(StatusItem::working("ra starting…", start, None)));
        progress.initialized(start + Duration::from_secs(1));
        assert_eq!(progress.status("ra"), None);
        assert_eq!(progress.take_ready(), Some(Duration::from_secs(1)));
        assert_eq!(progress.take_ready(), None);
    }

    #[test]
    fn reported_work_shows_with_its_percent_until_it_ends() {
        let start = Instant::now();
        let mut progress = Progress::new(start, false);
        progress.initialized(start);
        assert_eq!(progress.status("ra"), Some(StatusItem::working("ra loading…", start, None)));
        let begin = json!({ "kind": "begin", "title": "Indexing", "percentage": 0 });
        progress.report(&progress_params("1", begin), start);
        let report = json!({ "kind": "report", "message": "3/12 (core)", "percentage": 25 });
        progress.report(&progress_params("1", report), start);
        let indexing = StatusItem::working("ra: Indexing 25%", start, Some(25));
        assert_eq!(progress.status("ra"), Some(indexing));
        let same = json!({ "kind": "report", "message": "4/12 (std)", "percentage": 25 });
        assert!(!progress.report(&progress_params("1", same), start), "nothing shown changed");
        assert!(progress.settle(true, start), "waiting requests are asked again");
        assert_eq!(progress.status("ra").map(|item| item.text), Some("ra: Indexing 25%".into()));
        progress.report(
            &progress_params("1", json!({ "kind": "end" })),
            start + Duration::from_secs(9),
        );
        assert_eq!(progress.status("ra"), None);
        assert_eq!(progress.take_ready(), Some(Duration::from_secs(9)));
    }

    #[test]
    fn work_after_the_first_ready_shows_from_when_it_began_and_is_not_ready_again() {
        let start = Instant::now();
        let mut progress = Progress::new(start, true);
        progress.initialized(start);
        progress.take_ready();
        let later = start + Duration::from_secs(5);
        progress
            .report(&progress_params("2", json!({ "kind": "begin", "title": "Building" })), later);
        assert_eq!(progress.status("ra"), Some(StatusItem::working("ra: Building", later, None)));
        progress.report(&progress_params("2", json!({ "kind": "end" })), later);
        assert_eq!(progress.take_ready(), None);
    }

    #[test]
    fn without_a_percent_only_the_counter_of_a_message_shows() {
        let start = Instant::now();
        let mut progress = Progress::new(start, true);
        progress.initialized(start);
        let begin =
            json!({ "kind": "begin", "title": "Roots Scanned", "message": "3/40: /a/b.rs" });
        progress.report(&progress_params("3", begin), start);
        let scanned = StatusItem::working("ra: Roots Scanned 3/40", start, None);
        assert_eq!(progress.status("ra"), Some(scanned.clone()));
        progress.report(&progress_params("3", json!({ "kind": "report" })), start);
        assert_eq!(progress.status("ra"), Some(scanned));
        let paths = json!({ "kind": "report", "message": "/a/c.rs" });
        progress.report(&progress_params("3", paths), start);
        let titled = StatusItem::working("ra: Roots Scanned", start, None);
        assert_eq!(progress.status("ra"), Some(titled));
    }

    #[test]
    fn a_counter_is_two_numbers_around_a_slash() {
        assert_eq!(counter("536/635 (regalloc2 + 7 more)"), Some("536/635".to_owned()));
        assert_eq!(counter("12/300: /Users/me/x.rs"), Some("12/300".to_owned()));
        assert_eq!(counter("/Users/me/x.rs"), None);
        assert_eq!(counter("crates/app"), None);
        assert_eq!(counter(""), None);
    }
}
