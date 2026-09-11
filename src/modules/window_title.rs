use crate::{
    config::{WindowTitleMode, WindowTitleModuleConfig},
    services::{ReadOnlyService, ServiceEvent, compositor::CompositorService},
    theme::use_theme,
    utils::truncate_text,
};
use iced::{
    Element, Subscription,
    widget::{container, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    Event(Box<ServiceEvent<CompositorService>>),
    ConfigReloaded(WindowTitleModuleConfig),
}

pub struct WindowTitle {
    config: WindowTitleModuleConfig,
    service: Option<CompositorService>,
    value: Option<String>,
}

impl WindowTitle {
    pub fn new(config: WindowTitleModuleConfig) -> Self {
        Self {
            config,
            service: None,
            value: None,
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Event(event) => {
                if matches!(
                    event.apply(&mut self.service),
                    crate::services::Applied::Init | crate::services::Applied::Updated
                ) {
                    self.recalculate_value();
                }
            }
            Message::ConfigReloaded(cfg) => {
                self.config = cfg;
                self.recalculate_value();
            }
        }
    }

    fn recalculate_value(&mut self) {
        if let Some(service) = &self.service {
            self.value = service.active_window.as_ref().map(|w| {
                let raw_title = match self.config.mode {
                    WindowTitleMode::Title => w.title(),
                    WindowTitleMode::Class => w.class(),
                    WindowTitleMode::InitialTitle => match w.initial_title() {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!("{}", e);
                            ""
                        }
                    },
                    WindowTitleMode::InitialClass => match w.initial_class() {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!("{}", e);
                            ""
                        }
                    },
                };

                // Apply hard limit of 2048 characters to prevent Wayland E2BIG errors
                let max_length = if self.config.truncate_title_after_length > 0 {
                    std::cmp::min(self.config.truncate_title_after_length, 2048)
                } else {
                    2048
                };

                truncate_text(raw_title, max_length)
            });
        }
    }

    pub fn view(&'_ self) -> Option<Element<'_, Message>> {
        self.value.as_ref().map(|title| {
            use_theme(|theme| {
                container(
                    text(title.as_str())
                        .size(theme.font_size.sm)
                        .wrapping(text::Wrapping::None),
                )
                .clip(true)
                .into()
            })
        })
    }

    pub fn subscription(&self) -> Subscription<Message> {
        CompositorService::subscribe().map(|event| Message::Event(Box::new(event)))
    }
}
