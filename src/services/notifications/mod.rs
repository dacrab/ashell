use super::impl_service_subscription;
use crate::services::{ReadOnlyService, ServiceEvent};
use iced::futures::{SinkExt, StreamExt, channel::mpsc::Sender};
use iced::widget::{image, svg};
use log::{error, info};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_stream::wrappers::BroadcastStream;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, zvariant};

pub mod dbus;

use dbus::NotificationEvent;
pub use dbus::{Notification, Urgency};

#[derive(Debug, Clone)]
pub enum NotificationIcon {
    Image(image::Handle),
    Svg(svg::Handle),
}

impl NotificationIcon {
    pub fn resolve(
        app_name: &str,
        app_icon: &str,
        hints: &HashMap<String, OwnedValue>,
    ) -> Option<Self> {
        try_icon_from_hints(hints).or_else(|| {
            icon_candidates(app_name, app_icon, hints)
                .find_map(resolve_candidate)
                .map(Self::from_path)
        })
    }

    fn from_path(path: PathBuf) -> Self {
        let is_svg = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
        if is_svg {
            Self::Svg(svg::Handle::from_path(path))
        } else {
            Self::Image(image::Handle::from_path(path))
        }
    }
}

// freedesktop notification hint image data type
#[derive(zvariant::OwnedValue, Debug)]
struct HintImageData {
    width: i32,
    height: i32,
    rowstride: i32,
    has_alpha: bool,
    bits_per_sample: i32,
    channels: i32,
    image_bytes: Vec<u8>,
}

fn try_icon_from_hints(hints: &HashMap<String, OwnedValue>) -> Option<NotificationIcon> {
    if let Some(image_data) = hints
        .get("image-data")
        .or(hints.get("image_data"))
        .or(hints.get("icon_data"))
        && let Ok(hint_image_data) = HintImageData::try_from(image_data.clone())
        && let width = hint_image_data.width
        && width > 0
        && let height = hint_image_data.height
        && height > 0
        && let rowstride = hint_image_data.rowstride
        && rowstride >= width * hint_image_data.channels
        && let Some(bytes) = if hint_image_data.has_alpha && hint_image_data.channels == 4 {
            destride_rgba(&hint_image_data.image_bytes, width, height, rowstride)
        } else if !hint_image_data.has_alpha && hint_image_data.channels == 3 {
            Some(
                hint_image_data
                    .image_bytes
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|[r, g, b]| [*r, *g, *b, 255_u8])
                    .collect(),
            )
        } else {
            None
        }
        && bytes.len() == (width * height * 4) as usize
    {
        Some(NotificationIcon::Image(
            iced::advanced::image::Handle::from_rgba(width as u32, height as u32, bytes),
        ))
    } else {
        None
    }
}

// The spec allows rowstride >= width*4 (row padding); copy row-by-row so
// padded buffers render instead of being dropped by a strict length check.
fn destride_rgba(bytes: &[u8], width: i32, height: i32, rowstride: i32) -> Option<Vec<u8>> {
    let row = (width * 4) as usize;
    let stride = rowstride as usize;
    let out_len = row.checked_mul(height as usize)?;
    if bytes.len() < stride.checked_mul(height as usize)? {
        return None;
    }
    let mut out = Vec::with_capacity(out_len);
    for y in 0..height as usize {
        let start = y * stride;
        out.extend_from_slice(&bytes[start..start + row]);
    }
    Some(out)
}

const HINT_KEYS: &[&str] = &[
    "image-path",
    "image_path",
    "icon-name",
    "icon_name",
    "desktop-entry",
];

fn icon_candidates<'a>(
    app_name: &'a str,
    app_icon: &'a str,
    hints: &'a HashMap<String, OwnedValue>,
) -> impl Iterator<Item = String> + 'a {
    std::iter::once(app_icon.to_string())
        .chain(
            HINT_KEYS
                .iter()
                .filter_map(|k| hints.get(*k).and_then(|v| v.clone().try_into().ok())),
        )
        .chain(std::iter::once(app_name.to_string()))
        .map(|s: String| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_candidate(candidate: String) -> Option<PathBuf> {
    if let Ok(url) = url::Url::parse(&candidate)
        && url.scheme() == "file"
    {
        return url.to_file_path().ok().filter(|p| p.exists());
    }

    if candidate.contains('/') || candidate.starts_with('.') {
        let path = PathBuf::from(&candidate);
        if path.exists() {
            return Some(path);
        }
    }

    let name = candidate.strip_suffix(".desktop").unwrap_or(&candidate);
    freedesktop_lookup(name)
}

fn freedesktop_lookup(name: &str) -> Option<PathBuf> {
    crate::services::xdg_icons::find_icon_path(name)
}

#[derive(Debug, Clone)]
pub struct NotificationsService {
    pub connection: Connection,
}

impl NotificationsService {
    async fn init_service() -> anyhow::Result<(Connection, broadcast::Sender<NotificationEvent>)> {
        let (connection, event_tx) = dbus::NotificationDaemon::start_server().await?;
        Ok((connection, event_tx))
    }

    async fn start_listening(state: State, output: &mut Sender<ServiceEvent<Self>>) -> State {
        match state {
            State::Init => match Self::init_service().await {
                Ok((connection, event_tx)) => {
                    info!("Notifications service initialized");
                    let _ = output
                        .send(ServiceEvent::Init(NotificationsService {
                            connection: connection.clone(),
                        }))
                        .await;
                    State::Active(connection, event_tx)
                }
                Err(err) => {
                    error!("Failed to initialize notifications service: {err}");
                    State::Error
                }
            },
            State::Active(_connection, event_tx) => {
                let rx = event_tx.subscribe();
                let mut stream = BroadcastStream::new(rx);

                while let Some(result) = stream.next().await {
                    match result {
                        Ok(event) => {
                            let _ = output.send(ServiceEvent::Update(event)).await;
                        }
                        Err(e) => {
                            error!("Error receiving notification event: {e}");
                        }
                    }
                }
                error!("Notification event stream ended unexpectedly");
                State::Error
            }
            State::Error => {
                error!("Notifications service error, retrying in 5 seconds");

                sleep(Duration::from_secs(5)).await;

                State::Init
            }
        }
    }
}

enum State {
    Init,
    Active(Connection, broadcast::Sender<NotificationEvent>),
    Error,
}

impl ReadOnlyService for NotificationsService {
    type UpdateEvent = NotificationEvent;
    type Error = ();

    fn update(&mut self, _event: NotificationEvent) {}

    impl_service_subscription!(NotificationsService, 100);
}
