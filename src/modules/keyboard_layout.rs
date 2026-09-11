use crate::{
    config::KeyboardLayoutModuleConfig,
    services::{
        ReadOnlyService, Service, ServiceEvent,
        compositor::{CompositorCommand, CompositorService},
    },
};
use iced::{Element, Subscription, Task, widget::text};

#[derive(Debug, Clone)]
pub enum Message {
    Event(Box<ServiceEvent<CompositorService>>),
    ChangeLayout,
    ConfigReloaded(KeyboardLayoutModuleConfig),
}

pub struct KeyboardLayout {
    config: KeyboardLayoutModuleConfig,
    service: Option<CompositorService>,
}

impl KeyboardLayout {
    pub fn new(config: KeyboardLayoutModuleConfig) -> Self {
        Self {
            config,
            service: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Event(event) => {
                event.apply(&mut self.service);
                Task::none()
            }
            Message::ChangeLayout => {
                if let Some(service) = &mut self.service {
                    return service
                        .command(CompositorCommand::NextLayout)
                        .map(|event| Message::Event(Box::new(event)));
                }
                Task::none()
            }
            Message::ConfigReloaded(new_config) => {
                self.config = new_config;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Option<Element<'_, Message>> {
        let service = self.service.as_ref()?;
        let active_layout = &service.keyboard_layout;

        // Hide the module when the backend reports no layout (e.g. generic Wayland).
        if active_layout.is_empty() {
            return None;
        }

        // Fallback to displaying the layout ID/Name if no label config exists
        let label: &str = self
            .config
            .labels
            .get(active_layout)
            .map_or(active_layout.as_str(), String::as_str);

        Some(text(label).into())
    }

    pub fn subscription(&self) -> Subscription<Message> {
        CompositorService::subscribe().map(|event| Message::Event(Box::new(event)))
    }
}
