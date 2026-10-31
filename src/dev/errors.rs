//! The app's recent errors, kept in memory only, for a dev note's "Include recent errors". Pages
//! record the errors they show; `record_panics` adds panics.
use chrono::{DateTime, Local, TimeDelta};
use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
    time::Duration,
};

const KEEP: usize = 100;
/// A note lists at most this many different errors.
const SHOWN: usize = 10;

type Entry = (DateTime<Local>, &'static str, String);
static LOG: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());

/// An error as a note lists it: its latest time, and how often it happened.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub at: DateTime<Local>,
    pub origin: &'static str,
    pub message: String,
    pub count: usize,
}

pub fn record(origin: &'static str, message: impl Into<String>) {
    let mut log = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    if log.len() == KEEP {
        log.pop_front();
    }
    log.push_back((Local::now(), origin, message.into()));
}

/// The errors of the last `within`, newest first; a repeated error is listed once, counted.
pub fn recent(within: Duration) -> Vec<Recent> {
    let log = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    recent_in(&log, Local::now(), within)
}

fn recent_in(log: &VecDeque<Entry>, now: DateTime<Local>, within: Duration) -> Vec<Recent> {
    let since = now - TimeDelta::from_std(within).unwrap_or(TimeDelta::MAX);
    let mut out: Vec<Recent> = Vec::new();
    for (at, origin, message) in log.iter().rev().filter(|(at, ..)| *at >= since) {
        if let Some(seen) = out
            .iter_mut()
            .find(|seen| seen.origin == *origin && seen.message == *message)
        {
            seen.count += 1;
        } else if out.len() < SHOWN {
            out.push(Recent {
                at: *at,
                origin,
                message: message.clone(),
                count: 1,
            });
        }
    }
    out
}

/// Records each panic, then lets the default hook report it.
pub fn record_panics() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record("panic", info.to_string());
        report(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_errors_are_newest_first_counted_and_bounded() {
        let now = Local::now();
        let ago = |minutes| now - TimeDelta::minutes(minutes);
        let mut log: VecDeque<Entry> = [
            (ago(30), "update", "too old".to_string()),
            (ago(9), "download", "HTTP 403".to_string()),
            (ago(5), "gif", "No video stream.".to_string()),
            (ago(2), "download", "HTTP 403".to_string()),
        ]
        .into();
        let recent = recent_in(&log, now, Duration::from_secs(15 * 60));
        assert_eq!(
            recent,
            vec![
                Recent {
                    at: ago(2),
                    origin: "download",
                    message: "HTTP 403".into(),
                    count: 2
                },
                Recent {
                    at: ago(5),
                    origin: "gif",
                    message: "No video stream.".into(),
                    count: 1
                },
            ]
        );
        log.extend((0..20).map(|n| (ago(1), "download", format!("error {n}"))));
        let recent = recent_in(&log, now, Duration::from_secs(15 * 60));
        assert_eq!(recent.len(), SHOWN);
        assert_eq!(recent[0].message, "error 19");
    }
}
