//! The bar's module registry: `module_view` maps a [`ModuleName`] to its
//! view and `OnModulePress` action, `module_subscription` to its
//! subscription. To add a module: add a `ModuleName` variant (plus its
//! deserialization in `config.rs`), then wire it into both matches and into
//! the `Message` enum in `app/message.rs`.

use crate::{
    app::{App, Message},
    components::animated_size,
    components::menu::MenuType,
    components::{module_group, module_item},
    config::{ModuleDef, ModuleName},
    theme::use_theme,
};
use iced::{Alignment, Element, Length, Subscription, SurfaceId, widget::Row};

pub mod custom_module;
pub mod keyboard_layout;
pub mod keyboard_submap;
pub mod media_player;
pub mod notifications;
pub mod privacy;
pub mod settings;
pub mod system_info;
pub mod tempo;
pub mod tray;
pub mod updates;
pub mod window_title;
pub mod workspaces;

// `Action(Message)` dominates real-world usage and `Message` is inherently
// large; the remaining variants are cheap. The size difference is acceptable
// because `OnModulePress` lives only briefly in view-building code.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum OnModulePress {
    Action(Message),
    ToggleMenu(MenuType),
    /// Extra mouse bindings for a menu-opening module. `Message` is a large
    /// enum, so the three optional handlers are boxed together.
    ToggleMenuWithExtra {
        menu_type: MenuType,
        extra: Box<ToggleMenuExtra>,
    },
    /// `Message` is a large enum, so the many-field variant is boxed.
    CustomAction(Box<CustomModuleAction>),
}

#[derive(Debug, Clone)]
pub struct ToggleMenuExtra {
    pub on_right_press: Option<Message>,
    pub on_scroll_up: Option<Message>,
    pub on_scroll_down: Option<Message>,
}

#[derive(Debug, Clone)]
pub struct CustomModuleAction {
    pub on_press: Message,
    pub on_right_press: Option<Message>,
    pub on_middle_press: Option<Message>,
    pub on_scroll_up: Option<Message>,
    pub on_scroll_down: Option<Message>,
}

impl App {
    pub fn modules_section<'a>(&'a self, id: SurfaceId) -> [Element<'a, Message>; 3] {
        let space = use_theme(|t| t.space);
        [
            &self.general_config.modules.left,
            &self.general_config.modules.center,
            &self.general_config.modules.right,
        ]
        .map(|modules_def| {
            let mut row = Row::with_capacity(modules_def.len())
                .height(Length::Shrink)
                .align_y(Alignment::Center)
                .spacing(space.xxs);

            for module_def in modules_def {
                row = row.push(match module_def {
                    ModuleDef::Single(module) => self.single_module_wrapper(id, module),
                    ModuleDef::Group(group) => self.group_module_wrapper(id, group),
                });
            }

            row.into()
        })
    }

    pub fn modules_subscriptions(&self, modules_def: &[ModuleDef]) -> Vec<Subscription<Message>> {
        let mut subscriptions = Vec::new();
        for module_def in modules_def {
            match module_def {
                ModuleDef::Single(module) => {
                    subscriptions.extend(self.module_subscription(module));
                }
                ModuleDef::Group(group) => {
                    subscriptions.extend(
                        group
                            .iter()
                            .filter_map(|module| self.module_subscription(module)),
                    );
                }
            }
        }
        subscriptions
    }

    fn build_module_item<'a>(
        &'a self,
        id: SurfaceId,
        content: Element<'a, Message>,
        action: Option<OnModulePress>,
    ) -> Element<'a, Message> {
        let content = if use_theme(|t| t.animations_enabled) {
            animated_size(content).into()
        } else {
            content
        };
        match action {
            Some(action) => {
                let mut item = module_item(content);
                match action {
                    OnModulePress::Action(msg) => {
                        item = item.on_press(msg);
                    }
                    OnModulePress::ToggleMenu(menu_type) => {
                        item = item.on_press_with_position(move |button_ui_ref| {
                            Message::ToggleMenu(menu_type.clone(), id, button_ui_ref)
                        });
                    }
                    OnModulePress::ToggleMenuWithExtra { menu_type, extra } => {
                        item = item.on_press_with_position(move |button_ui_ref| {
                            Message::ToggleMenu(menu_type.clone(), id, button_ui_ref)
                        });
                        if let Some(msg) = extra.on_right_press {
                            item = item.on_right_press(msg);
                        }
                        if let Some(msg) = extra.on_scroll_up {
                            item = item.on_scroll_up(msg);
                        }
                        if let Some(msg) = extra.on_scroll_down {
                            item = item.on_scroll_down(msg);
                        }
                    }
                    OnModulePress::CustomAction(action) => {
                        item = item.on_press(action.on_press);
                        if let Some(msg) = action.on_right_press {
                            item = item.on_right_press(msg);
                        }
                        if let Some(msg) = action.on_middle_press {
                            item = item.on_middle_press(msg);
                        }
                        if let Some(msg) = action.on_scroll_up {
                            item = item.on_scroll_up(msg);
                        }
                        if let Some(msg) = action.on_scroll_down {
                            item = item.on_scroll_down(msg);
                        }
                    }
                }
                item.into()
            }
            None => module_item(content).into(),
        }
    }

    fn single_module_wrapper<'a>(
        &'a self,
        id: SurfaceId,
        module_name: &'a ModuleName,
    ) -> Option<Element<'a, Message>> {
        self.module_view(id, module_name)
            .map(|(content, action)| module_group(self.build_module_item(id, content, action)))
    }

    fn group_module_wrapper<'a>(
        &'a self,
        id: SurfaceId,
        group: &'a [ModuleName],
    ) -> Option<Element<'a, Message>> {
        let modules: Vec<_> = group
            .iter()
            .filter_map(|module| self.module_view(id, module))
            .collect();

        if modules.is_empty() {
            None
        } else {
            let items = Row::with_children(
                modules
                    .into_iter()
                    .map(|(content, action)| self.build_module_item(id, content, action))
                    .collect::<Vec<_>>(),
            );
            Some(module_group(items.into()))
        }
    }

    fn module_view<'a>(
        &'a self,
        id: SurfaceId,
        module_name: &'a ModuleName,
    ) -> Option<(Element<'a, Message>, Option<OnModulePress>)> {
        match module_name {
            ModuleName::Custom(name) => self.custom.get(name).map(|custom| {
                let action = match custom.module_type() {
                    crate::config::CustomModuleType::Text => None,
                    crate::config::CustomModuleType::Button => {
                        let name = name.clone();
                        Some(OnModulePress::CustomAction(Box::new(CustomModuleAction {
                            on_press: Message::Custom(
                                name.clone(),
                                custom_module::Message::LaunchCommand,
                            ),
                            on_right_press: custom.config.on_right_click.as_ref().map(|_| {
                                Message::Custom(
                                    name.clone(),
                                    custom_module::Message::LaunchRightClickCommand,
                                )
                            }),
                            on_middle_press: custom.config.on_middle_click.as_ref().map(|_| {
                                Message::Custom(
                                    name.clone(),
                                    custom_module::Message::LaunchMiddleClickCommand,
                                )
                            }),
                            on_scroll_up: custom.config.on_scroll_up.as_ref().map(|_| {
                                Message::Custom(
                                    name.clone(),
                                    custom_module::Message::LaunchScrollUpCommand,
                                )
                            }),
                            on_scroll_down: custom.config.on_scroll_down.as_ref().map(|_| {
                                Message::Custom(
                                    name,
                                    custom_module::Message::LaunchScrollDownCommand,
                                )
                            }),
                        })))
                    }
                };
                (
                    custom.view().map(|msg| Message::Custom(name.clone(), msg)),
                    action,
                )
            }),
            ModuleName::Updates => self.updates.as_ref().map(|updates| {
                (
                    updates.view().map(Message::Updates),
                    Some(OnModulePress::ToggleMenu(MenuType::Updates)),
                )
            }),
            ModuleName::Workspaces => Some((
                self.workspaces
                    .view(id, &self.outputs)
                    .map(Message::Workspaces),
                None,
            )),
            ModuleName::WindowTitle => self
                .window_title
                .view()
                .map(|view| (view.map(Message::WindowTitle), None)),
            ModuleName::SystemInfo => Some((
                self.system_info.view().map(Message::SystemInfo),
                Some(OnModulePress::ToggleMenu(MenuType::SystemInfo)),
            )),
            ModuleName::KeyboardLayout => self.keyboard_layout.view().map(|view| {
                (
                    view.map(Message::KeyboardLayout),
                    Some(OnModulePress::Action(Message::KeyboardLayout(
                        keyboard_layout::Message::ChangeLayout,
                    ))),
                )
            }),
            ModuleName::KeyboardSubmap => self
                .keyboard_submap
                .view()
                .map(|view| (view.map(Message::KeyboardSubmap), None)),
            ModuleName::Tray => self
                .tray
                .view(id)
                .map(|view| (view.map(Message::Tray), None)),
            ModuleName::Tempo => Some((
                self.tempo.view().map(Message::Tempo),
                Some(OnModulePress::ToggleMenuWithExtra {
                    menu_type: MenuType::Tempo,
                    extra: Box::new(ToggleMenuExtra {
                        on_right_press: Some(Message::Tempo(tempo::Message::CycleFormat)),
                        on_scroll_up: Some(Message::Tempo(tempo::Message::CycleTimezone(
                            tempo::TimezoneDirection::Forward,
                        ))),
                        on_scroll_down: Some(Message::Tempo(tempo::Message::CycleTimezone(
                            tempo::TimezoneDirection::Backward,
                        ))),
                    }),
                }),
            )),
            ModuleName::Privacy => self
                .privacy
                .view()
                .map(|view| (view.map(Message::Privacy), None)),
            ModuleName::MediaPlayer => self.media_player.view().map(|view| {
                (
                    view.map(Message::MediaPlayer),
                    Some(OnModulePress::ToggleMenu(MenuType::MediaPlayer)),
                )
            }),
            ModuleName::Settings => Some((
                self.settings.view(id).map(Message::Settings),
                Some(OnModulePress::ToggleMenu(MenuType::Settings)),
            )),
            ModuleName::Notifications => Some((
                self.notifications.view().map(Message::Notifications),
                Some(OnModulePress::ToggleMenu(MenuType::Notifications)),
            )),
        }
    }

    fn module_subscription(&self, module_name: &ModuleName) -> Option<Subscription<Message>> {
        match module_name {
            ModuleName::Custom(name) => self.custom.get(name).map(|custom| {
                custom
                    .subscription()
                    .map(|(name, msg)| Message::Custom(name, msg))
            }),
            ModuleName::Updates => self
                .updates
                .as_ref()
                .map(|updates| updates.subscription().map(Message::Updates)),
            ModuleName::Workspaces => Some(self.workspaces.subscription().map(Message::Workspaces)),
            ModuleName::WindowTitle => {
                Some(self.window_title.subscription().map(Message::WindowTitle))
            }
            ModuleName::SystemInfo => {
                Some(self.system_info.subscription().map(Message::SystemInfo))
            }
            ModuleName::KeyboardLayout => Some(
                self.keyboard_layout
                    .subscription()
                    .map(Message::KeyboardLayout),
            ),
            ModuleName::KeyboardSubmap => Some(
                self.keyboard_submap
                    .subscription()
                    .map(Message::KeyboardSubmap),
            ),
            ModuleName::Tray => Some(self.tray.subscription().map(Message::Tray)),
            ModuleName::Tempo => Some(self.tempo.subscription().map(Message::Tempo)),
            ModuleName::Privacy => Some(self.privacy.subscription().map(Message::Privacy)),
            ModuleName::MediaPlayer => Some(
                self.media_player
                    .subscription(self.outputs.menu_of_type_is_open(&MenuType::MediaPlayer))
                    .map(Message::MediaPlayer),
            ),
            ModuleName::Settings => Some(self.settings.subscription().map(Message::Settings)),
            ModuleName::Notifications => Some(
                self.notifications
                    .subscription()
                    .map(Message::Notifications),
            ),
        }
    }
}
