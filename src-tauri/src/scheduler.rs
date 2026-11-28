//! Background refresh. Checks once a minute which feeds are due.

use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

use crate::db;
use crate::feeds;
use crate::state::AppState;

pub fn start(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        housekeeping(&state);
        if state.settings().refresh_on_startup {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let ids = due_feeds(&state, true);
            feeds::refresh_many(&app, &state, ids).await;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.tick().await;
        let mut minutes: u64 = 0;
        loop {
            tick.tick().await;
            minutes += 1;
            let ids = due_feeds(&state, false);
            feeds::refresh_many(&app, &state, ids).await;
            if minutes % (6 * 60) == 0 {
                housekeeping(&state);
            }
        }
    });
}

fn due_feeds(state: &AppState, startup: bool) -> Vec<i64> {
    let settings = state.settings();
    let now = db::now();
    let Ok(schedules) = db::schedules(&state.db()) else { return Vec::new() };
    schedules
        .into_iter()
        .filter(|s| !s.paused)
        .filter(|s| {
            let interval = s.update_interval.unwrap_or(settings.refresh_interval as i64);
            if startup {
                // "Refresh on startup" covers every feed that is not manual-only.
                return interval > 0 || s.update_interval.is_none();
            }
            is_due(s, interval, now)
        })
        .map(|s| s.id)
        .collect()
}

/// The shortest wait before a feed that has never loaded is tried again.
const FIRST_RETRY: i64 = 5 * 60;

/// Whether a feed is due at `now`. `interval` is its refresh interval in minutes;
/// 0 means it is refreshed only by hand.
fn is_due(s: &db::Schedule, interval: i64, now: i64) -> bool {
    if interval <= 0 {
        return false;
    }
    let Some(fetched) = s.last_fetched else { return true };
    let mut wait = interval * 60;
    // A feed that has never loaded, such as a new feed whose server failed at the
    // first try (YouTube's feed server does this for hours): try again sooner. The
    // wait is the feed's age at the last try, at least 5 minutes, so it doubles:
    // 5, 5, 10, 20, 40 minutes, until it reaches the interval. A `created_at` after
    // the last try (from a crafted backup) gets the usual interval.
    if s.last_success.is_none() && s.created_at <= fetched {
        wait = wait.min((fetched - s.created_at).max(FIRST_RETRY));
    }
    now - fetched >= wait
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60;

    fn feed(created_at: i64, last_fetched: Option<i64>, last_success: Option<i64>) -> db::Schedule {
        db::Schedule { id: 1, update_interval: None, last_fetched, last_success, created_at, paused: false }
    }

    /// The minutes after `fetched` at which the feed is first due.
    fn next_try(s: &db::Schedule, interval: i64) -> i64 {
        let fetched = s.last_fetched.unwrap();
        (0..=interval * 2).find(|m| is_due(s, interval, fetched + m * MINUTE)).unwrap()
    }

    #[test]
    fn feeds_that_loaded_wait_their_interval() {
        let s = feed(0, Some(1000 * MINUTE), Some(1000 * MINUTE));
        assert_eq!(next_try(&s, 60), 60);
        assert!(is_due(&feed(0, None, None), 60, 0), "a feed never tried is due");
        assert!(!is_due(&feed(0, Some(0), None), 0, 1000 * MINUTE), "a manual-only feed is never due");
    }

    #[test]
    fn a_new_feed_that_failed_is_tried_again_sooner() {
        let t0 = 1_000_000;
        // Tried when added, then at each time it became due: the waits double.
        let mut fetched = t0;
        let mut waits = Vec::new();
        for _ in 0..7 {
            let wait = next_try(&feed(t0, Some(fetched), None), 60);
            waits.push(wait);
            fetched += wait * MINUTE;
        }
        assert_eq!(waits, [5, 5, 10, 20, 40, 60, 60]);
        // A short interval stays short.
        assert_eq!(next_try(&feed(t0, Some(t0), None), 2), 2);
        // Once it has loaded, the usual interval again.
        assert_eq!(next_try(&feed(t0, Some(t0 + 10 * MINUTE), Some(t0 + 10 * MINUTE)), 60), 60);
        // A creation time after the last try does not make it retry every 5 minutes.
        assert_eq!(next_try(&feed(t0 + 999 * MINUTE, Some(t0), None), 60), 60);
    }
}

/// Applies archive limits to all feeds and removes old tombstones.
fn housekeeping(state: &AppState) {
    let settings = state.settings();
    let conn = state.db();
    if let Ok(feeds) = db::feed_retention(&conn) {
        for (id, mode, value) in feeds {
            let (mode, value) = feeds::retention_for(mode.as_deref(), value, &settings);
            let _ = db::apply_retention(&conn, id, &mode, value);
        }
    }
    let _ = db::purge_tombstones(&conn);
    db::checkpoint(&conn);
    let _ = conn.execute_batch("PRAGMA optimize;");
}
