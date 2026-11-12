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
            interval > 0 && s.last_fetched.is_none_or(|t| now - t >= interval * 60)
        })
        .map(|s| s.id)
        .collect()
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
