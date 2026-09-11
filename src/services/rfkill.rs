//! rfkill helpers shared by the bluetooth and network services.
//!
//! These run the `rfkill` binary from `PATH` (not a hardcoded location) so
//! they work on distributions where it lives outside `/usr/sbin`.

use std::io::ErrorKind;
use std::pin::Pin;

use iced::futures::{Stream, StreamExt};
use inotify::{Inotify, WatchMask};
use log::{debug, warn};
use tokio::process::Command;

type EventStream = Pin<Box<dyn Stream<Item = ()> + Send>>;

async fn run(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("rfkill").args(args).output().await
}

/// Whether the bluetooth radio is currently soft blocked.
pub async fn check_soft_block() -> anyhow::Result<bool> {
    let output = match run(&["list", "bluetooth"]).await {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            warn!("rfkill binary not found, assuming bluetooth is not soft blocked");
            return Ok(false);
        }
        Err(err) => return Err(err.into()),
    };

    let output = String::from_utf8(output.stdout)?;

    Ok(output.contains("Soft blocked: yes"))
}

/// Block or unblock the bluetooth radio.
pub async fn set_block(blocked: bool) {
    let action = if blocked { "block" } else { "unblock" };
    if let Err(e) = run(&[action, "bluetooth"]).await {
        debug!("Failed to set bluetooth rfkill: {e}");
    } else {
        debug!("Bluetooth rfkill set successfully");
    }
}

/// Stream that emits whenever the rfkill state changes (via /dev/rfkill).
pub async fn listen_soft_block_changes() -> anyhow::Result<EventStream> {
    let inotify = Inotify::init()?;

    match inotify.watches().add("/dev/rfkill", WatchMask::MODIFY) {
        Ok(_) => {
            let buffer = [0; 512];
            Ok(inotify.into_event_stream(buffer)?.map(|_| {}).boxed())
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            warn!("/dev/rfkill not found, disabling rfkill change notifications for bluetooth");
            Ok(iced::futures::stream::pending().boxed())
        }
        Err(err) => Err(err.into()),
    }
}
