//! The on-screen display: a transient bottom-anchored overlay for volume,
//! brightness and toggle events (typically from IPC). Not a bar module —
//! `Action::Show`/`Hide` tell the app to create/destroy the overlay surface.

use std::time::Duration;

/// OSD progress bar dimensions.
const OSD_BAR_LENGTH: f32 = 160.0;
const OSD_BAR_GIRTH: f32 = 8.0;

use iced::{
    Alignment, Element, Length, Task, Theme,
    widget::{blur_container, container, progress_bar, row, text},
};
use tokio::time::sleep;

use crate::{
    components::icons::{Icon, StaticIcon},
    config::{OsdModuleConfig, Surface},
    modules::settings::audio::AudioSettings,
    modules::settings::network::NetworkSettings,
    services::idle_inhibitor::IdleInhibitorManager,
    t,
    theme::{surface_border, use_theme},
};

pub struct Osd {
    config: OsdModuleConfig,
    state: Option<OsdState>,
    timeout_handle: Option<iced::task::Handle>,
}

struct OsdState {
    kind: OsdKind,
    /// Normalised value relative to 100% hardware (can exceed 1.0 for overdrive)
    value: f32,
    scale: f32,
    muted: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum OsdKind {
    Volume,
    Microphone,
    Brightness,
    Airplane,
    IdleInhibitor,
}

#[derive(Debug, Clone)]
pub enum Message {
    Show {
        kind: OsdKind,
        value: f32,
        scale: f32,
        muted: bool,
    },
    Hide,
    ConfigReloaded(OsdModuleConfig),
}

pub enum Action {
    None,
    /// OSD state updated — caller should ensure the layer surface exists and
    /// run the returned timer task.
    Show(Task<Message>),
    /// Timer expired — caller must destroy the layer surface.
    Hide,
}

impl Osd {
    pub fn new(config: OsdModuleConfig) -> Self {
        Self {
            config,
            state: None,
            timeout_handle: None,
        }
    }

    pub fn config(&self) -> &OsdModuleConfig {
        &self.config
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Show {
                kind,
                value,
                scale,
                muted,
            } => {
                self.state = Some(OsdState {
                    kind,
                    value,
                    scale,
                    muted,
                });

                if let Some(handle) = self.timeout_handle.take() {
                    handle.abort();
                }

                let timeout_ms = self.config.timeout;
                let (task, handle) = Task::perform(
                    async move {
                        sleep(Duration::from_millis(timeout_ms)).await;
                    },
                    |()| Message::Hide,
                )
                .abortable();
                self.timeout_handle = Some(handle);

                Action::Show(task)
            }

            Message::Hide => {
                self.state = None;
                if let Some(handle) = self.timeout_handle.take() {
                    handle.abort();
                }
                Action::Hide
            }

            Message::ConfigReloaded(config) => {
                self.config = config;
                Action::None
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let Some(state) = &self.state else {
            return row![].into();
        };

        let (space, font_size, radius, blur) =
            use_theme(|t| (t.space, t.font_size, t.radius, t.surface(Surface::Osd).blur));

        let overdrive = matches!(state.kind, OsdKind::Volume) && state.value > 1.0;

        let icon = match state.kind {
            OsdKind::Volume => AudioSettings::speaker_icon(state.muted, state.value, overdrive),
            OsdKind::Microphone => AudioSettings::microphone_icon(state.muted),
            OsdKind::Brightness => StaticIcon::Brightness,
            OsdKind::Airplane => NetworkSettings::airplane_mode_icon(state.muted),
            OsdKind::IdleInhibitor => IdleInhibitorManager::idle_inhibitor_icon(state.muted),
        };

        let show_percentage = match state.kind {
            OsdKind::Volume | OsdKind::Microphone => self.config.show_volume_percentage,
            OsdKind::Brightness => self.config.show_brightness_percentage,
            _ => false,
        };

        let detail: Element<'_, Message> = match state.kind {
            OsdKind::Volume | OsdKind::Microphone | OsdKind::Brightness => {
                let bar = progress_bar(0.0..=state.scale, state.value)
                    .length(OSD_BAR_LENGTH)
                    .girth(OSD_BAR_GIRTH);
                let bar = if state.muted {
                    bar.style(crate::theme::progress_bar_secondary)
                } else if overdrive {
                    bar.style(crate::theme::progress_bar_danger)
                } else {
                    bar.style(crate::theme::progress_bar_primary)
                };
                if show_percentage {
                    let pct = (state.value * 100.0).round() as u32;
                    row![
                        container(bar).center_x(Length::Fill),
                        text(format!("{pct}%")).size(font_size.sm),
                    ]
                    .spacing(space.sm)
                    .align_y(Alignment::Center)
                    .into()
                } else {
                    container(bar).center_x(Length::Fill).into()
                }
            }
            OsdKind::Airplane | OsdKind::IdleInhibitor => {
                // For toggles, `muted` carries the active/enabled state.
                let state_key = if state.muted { "on" } else { "off" };
                let label = match state.kind {
                    OsdKind::Airplane => t!("osd-airplane-toggle", state = state_key),
                    OsdKind::IdleInhibitor => t!("osd-idle-inhibitor-toggle", state = state_key),
                    _ => unreachable!(),
                };
                container(text(label)).center_x(Length::Fill).into()
            }
        };

        let content = row![
            container(icon.to_text().size(font_size.xxl).center()).center_x(font_size.xxl),
            detail,
        ]
        .spacing(space.sm)
        .align_y(Alignment::Center);

        let osd_style = move |t: &Theme| container::Style {
            background: Some(t.palette().background.into()),
            border: surface_border(t, radius.xl),
            text_color: Some(match (state.kind, state.muted) {
                (OsdKind::IdleInhibitor, true) => t.palette().danger,
                (OsdKind::Airplane, true) => t.palette().danger,
                _ => t.palette().text,
            }),
            ..Default::default()
        };
        if blur {
            blur_container(content)
                .padding([space.sm, space.md])
                .style(osd_style)
                .center(Length::Fill)
                .into()
        } else {
            container(content)
                .padding([space.sm, space.md])
                .style(osd_style)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        }
    }
}
