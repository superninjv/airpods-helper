use tokio::sync::watch;
use tracing::{debug, error, info};
use zbus::Connection;

use crate::state::AirPodsState;

/// Watch ear detection state and pause/resume MPRIS media players
pub async fn watch_ear_detection(mut state_rx: watch::Receiver<AirPodsState>) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to connect to session bus for MPRIS: {e}");
            return;
        }
    };

    let mut was_in_ear = false;
    let mut paused_player: Option<String> = None;

    loop {
        if state_rx.changed().await.is_err() {
            break;
        }

        let state = state_rx.borrow_and_update().clone();

        if !state.connected {
            was_in_ear = false;
            paused_player = None;
            continue;
        }

        let in_ear = state.ear_left || state.ear_right;

        // Transition: was in ear -> no longer in ear = pause
        if was_in_ear && !in_ear {
            if let Some(player) = find_playing_player(&conn).await {
                info!("ear detection: pausing {player}");
                let _ = call_mpris(&conn, &player, "Pause").await;
                paused_player = Some(player);
            }
        }

        // Transition: was not in ear -> now in ear = resume
        if !was_in_ear && in_ear {
            if let Some(player) = paused_player.take() {
                info!("ear detection: resuming {player}");
                let _ = call_mpris(&conn, &player, "Play").await;
            }
        }

        was_in_ear = in_ear;
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

        // Check playback status
        let player_proxy = zbus::Proxy::new(
            conn,
            name_str,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        .ok()?;

        if let Ok(status) = player_proxy.get_property::<String>("PlaybackStatus").await {
            if status == "Playing" {
                return Some(name_str.to_string());
            }
        }
    }

    None
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
