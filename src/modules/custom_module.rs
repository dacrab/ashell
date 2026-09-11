use crate::{
    components::icons::{DynamicIcon, StaticIcon, icon},
    config::CustomModuleDef,
    theme::use_theme,
    utils::launcher::execute_command,
};
use iced::widget::canvas;
use iced::{
    Element, Length, Subscription, Theme,
    stream::channel,
    widget::{Space, Stack, row, text},
};
use iced::{
    mouse::Cursor,
    widget::{
        canvas::{Cache, Geometry, Path, Program},
        container,
    },
};
use log::{error, info, warn};
use serde::Deserialize;
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct Custom {
    pub config: CustomModuleDef,
    data: CustomListenData,
    /// Icon and alert resolved from the regex maps on data change, so
    /// `view()` doesn't run regexes per frame.
    resolved_icon: Option<String>,
    resolved_alert: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CustomListenData {
    pub alt: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchCommand,
    LaunchRightClickCommand,
    LaunchMiddleClickCommand,
    LaunchScrollUpCommand,
    LaunchScrollDownCommand,
    Update(CustomListenData),
}

#[derive(Debug, Clone, Copy, Default)]
struct AlertIndicator;

impl<Message> Program<Message> for AlertIndicator {
    type State = Cache;

    fn draw(
        &self,
        cache: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let geometry = cache.draw(renderer, bounds.size(), |frame| {
            let center = frame.center();
            let radius = 2.0;
            let circle = Path::circle(center, radius);
            frame.fill(&circle, theme.palette().danger);
        });

        vec![geometry]
    }
}

impl Custom {
    pub fn new(config: CustomModuleDef) -> Self {
        let mut custom = Self {
            config,
            data: CustomListenData::default(),
            resolved_icon: None,
            resolved_alert: false,
        };
        custom.resolve();
        custom
    }

    /// Re-resolve the regex-driven icon and alert from the current data.
    fn resolve(&mut self) {
        self.resolved_icon = self.config.icons.as_ref().and_then(|icons_map| {
            icons_map
                .iter()
                .find(|(re, _)| re.is_match(&self.data.alt))
                .map(|(_, icon_str)| icon_str.clone())
        });
        self.resolved_alert = self
            .config
            .alert
            .as_ref()
            .is_some_and(|re| re.is_match(&self.data.alt));
    }

    pub fn module_type(&self) -> crate::config::CustomModuleType {
        self.config.r#type
    }

    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::LaunchCommand => {
                if let Some(cmd) = &self.config.command {
                    execute_command(cmd);
                }
            }
            Message::LaunchRightClickCommand => {
                if let Some(cmd) = &self.config.on_right_click {
                    execute_command(cmd);
                }
            }
            Message::LaunchMiddleClickCommand => {
                if let Some(cmd) = &self.config.on_middle_click {
                    execute_command(cmd);
                }
            }
            Message::LaunchScrollUpCommand => {
                if let Some(cmd) = &self.config.on_scroll_up {
                    execute_command(cmd);
                }
            }
            Message::LaunchScrollDownCommand => {
                if let Some(cmd) = &self.config.on_scroll_down {
                    execute_command(cmd);
                }
            }
            Message::Update(data) => {
                self.data = data;
                self.resolve();
            }
        }
    }

    pub fn view(&'_ self) -> Element<'_, Message> {
        let space = use_theme(|theme| theme.space);
        match self.config.r#type {
            crate::config::CustomModuleType::Text => self
                .data
                .text
                .as_deref()
                .filter(|text_content| !text_content.is_empty())
                .map(|text_content| text(text_content).into())
                .unwrap_or_else(|| Space::new().width(Length::Shrink).into()),
            crate::config::CustomModuleType::Button => {
                let icon_element = self.resolved_icon.as_ref().map_or_else(
                    || match &self.config.icon {
                        Some(text) => icon(DynamicIcon(text.clone())),
                        None => icon(StaticIcon::None),
                    },
                    |text| icon(DynamicIcon(text.clone())),
                );

                let padded_icon_container = container(icon_element).padding([0, 1]);

                let icon_with_alert = if self.resolved_alert {
                    let alert_canvas = canvas(AlertIndicator)
                        .width(Length::Fixed(space.xs))
                        .height(Length::Fixed(space.xs));

                    // Container to position the dot at the top-right
                    let alert_indicator_container = container(alert_canvas)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Right)
                        .align_y(iced::alignment::Vertical::Top);

                    Stack::new()
                        .push(padded_icon_container)
                        .push(alert_indicator_container)
                        .into()
                } else {
                    padded_icon_container.into()
                };

                if let Some(text_content) = self.data.text.as_deref().filter(|t| !t.is_empty()) {
                    row![icon_with_alert, text(text_content)]
                        .spacing(space.xs)
                        .into()
                } else {
                    icon_with_alert
                }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<(String, Message)> {
        let name = self.config.name.clone();
        if let Some(listen_cmd) = self.config.listen_cmd.clone() {
            Subscription::run_with((name, listen_cmd), |data| {
                let (name, listen_cmd) = data.clone();
                channel(10, async move |mut output| {
                    let command = Command::new("bash")
                        .arg("-c")
                        .arg(&listen_cmd)
                        .stdout(Stdio::piped())
                        .spawn();

                    match command {
                        Ok(mut child) => {
                            if let Some(stdout) = child.stdout.take() {
                                let mut reader = BufReader::new(stdout).lines();
                                let mut buf = String::new();

                                // Ensure the child process is spawned in the runtime so it can
                                // make progress on its own while we await for any output.
                                tokio::spawn(async move {
                                    match child.wait().await {
                                        Ok(status) => info!("child status was: {status}"),
                                        Err(e) => warn!("child process encountered an error: {e}"),
                                    }
                                });

                                while let Some(line) = reader.next_line().await.ok().flatten() {
                                    buf.push_str(&line);
                                    buf.push('\n');
                                    match serde_json::from_str::<CustomListenData>(&buf) {
                                        Ok(event) => {
                                            buf.clear();
                                            if let Err(e) = output
                                                .try_send((name.clone(), Message::Update(event)))
                                            {
                                                error!(
                                                    "Failed to send update for custom module '{name}': {e}"
                                                );
                                            }
                                        }
                                        Err(e) if e.is_eof() => {
                                            if buf.len() > 1 << 20 {
                                                warn!(
                                                    "custom module '{name}': dropping {} bytes of unterminated JSON",
                                                    buf.len()
                                                );
                                                buf.clear();
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "Failed to parse JSON for custom module '{name}': {e} (payload: {buf})"
                                            );
                                            buf.clear();
                                        }
                                    }
                                }
                            } else {
                                error!("Failed to capture stdout for command: {listen_cmd}");
                            }
                        }
                        Err(error) => {
                            error!("Failed to execute command: {error}");
                        }
                    }
                })
            })
        } else {
            Subscription::none()
        }
    }
}
