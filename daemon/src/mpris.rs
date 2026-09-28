use tokio::sync::watch;
use tracing::{debug, error, info};
use zbus::Connection;

use crate::config::{self, SharedConfig};
use crate::state::AirPodsState;

fn buds_in_ear(state: &AirPodsState) -> u8 {
    state.ear_left as u8 + state.ear_right as u8
}

/// Watch ear detection and pause/resume MPRIS players, like iOS does:
/// taking a bud out pauses; putting it back resumes — but only a player we
/// paused ourselves, so we never start media the user stopped on purpose.
pub async fn watch_ear_detection(
    mut state_rx: watch::Receiver<AirPodsState>,
    config: SharedConfig,
) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to connect to session bus for MPRIS: {e}");
            return;
        }
    };

    // None until we've seen the first ear report of this connection, so the
    // initial "0 → N buds" isn't mistaken for an insertion.
    let mut in_ear: Option<u8> = None;
    // (player, number of buds that were in when we paused)
    let mut paused: Option<(String, u8)> = None;

    while state_rx.changed().await.is_ok() {
        let state = state_rx.borrow_and_update().clone();
        if !state.connected {
            in_ear = None;
            paused = None;
            continue;
        }
        let now = buds_in_ear(&state);
        let Some(before) = in_ear.replace(now) else {
            continue;
        };
        if now == before {
            continue;
        }
        let (pause, resume) = config::read(&config, |c| {
            (c.ear_detection.pause_media, c.ear_detection.resume_media)
        });

        if now < before {
            if pause
                && paused.is_none()
                && let Some(player) = find_playing_player(&conn).await
            {
                info!("ear detection: bud removed, pausing {player}");
                if call_mpris(&conn, &player, "Pause").await.is_ok() {
                    paused = Some((player, before));
                }
            }
        } else if let Some((player, wanted)) = &paused
            && now >= *wanted
        {
            // Only if it's still paused: the user may have stopped it, or
            // started something else, while the bud was out.
            if resume && playback_status(&conn, player).await.as_deref() == Some("Paused") {
                info!("ear detection: bud back in, resuming {player}");
                let _ = call_mpris(&conn, player, "Play").await;
            }
            paused = None;
        }
    }
}

/// Find the first MPRIS player that is currently playing
async fn find_playing_player(conn: &Connection) -> Option<String> {
    let proxy = zbus::fdo::DBusProxy::new(conn).await.ok()?;
    let names = proxy.list_names().await.ok()?;

    for name in names {
        let name_str = name.as_str();
        if !name_str.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }
        let player_proxy = match zbus::Proxy::new(
            conn,
            name_str,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                debug!("skipping MPRIS player {name_str}: {e}");
                continue;
            }
        };
        if let Ok(status) = player_proxy.get_property::<String>("PlaybackStatus").await
            && status == "Playing"
        {
            return Some(name_str.to_string());
        }
    }

    None
}

async fn playback_status(conn: &Connection, player: &str) -> Option<String> {
    let proxy = zbus::Proxy::new(conn, player, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player")
        .await
        .ok()?;
    proxy.get_property::<String>("PlaybackStatus").await.ok()
}

/// Call a method on an MPRIS player
async fn call_mpris(conn: &Connection, player: &str, method: &str) -> zbus::Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        player,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .await?;
    proxy.call_noreply(method, &()).await?;
    Ok(())
}
