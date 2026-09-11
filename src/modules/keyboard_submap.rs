use crate::services::{ReadOnlyService, ServiceEvent, compositor::CompositorService};
use iced::{Element, Subscription, widget::text};

#[derive(Debug, Clone)]
pub enum Message {
    Event(Box<ServiceEvent<CompositorService>>),
}

#[derive(Debug, Clone, Default)]
pub struct KeyboardSubmap {
    service: Option<CompositorService>,
}

impl KeyboardSubmap {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Event(event) => {
                event.apply(&mut self.service);
            }
        }
    }

    pub fn view(&self) -> Option<Element<'_, Message>> {
        let submap = self.service.as_ref()?.submap.as_ref()?;

        if !submap.is_empty() {
            Some(text(submap).into())
        } else {
            None
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        CompositorService::subscribe().map(|event| Message::Event(Box::new(event)))
    }
}
