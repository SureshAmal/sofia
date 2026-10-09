use gpui_kit::component::{
    button::Button,
    input::{Textarea, TextareaState},
    switch::Switch,
};
use gpui_kit::*;
use sofia_config::{McpOAuth, McpServerConfig, McpTransport};
use std::{sync::mpsc, time::Duration};

pub struct McpPanel {
    servers: Vec<Entity<ServerCard>>,
    json: Entity<TextareaState>,
    status: String,
}
struct ServerCard {
    config: McpServerConfig,
    tools: Vec<String>,
    status: String,
    pending: bool,
    results: mpsc::Receiver<CardResult>,
    sender: mpsc::Sender<CardResult>,
}
enum CardResult {
    Tools(Result<Vec<String>, String>),
    OAuth(Result<McpOAuth, String>),
}
impl McpPanel {
    pub fn new(configs: &[McpServerConfig], window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            servers: configs
                .iter()
                .map(|config| cx.new(|cx| ServerCard::new(config.clone(), cx)))
                .collect(),
            json: cx.new(|cx| TextareaState::new(window, cx).rows(6)),
            status: String::new(),
        }
    }
    pub fn collect(&self, cx: &App) -> Result<Vec<McpServerConfig>, String> {
        Ok(self
            .servers
            .iter()
            .map(|server| server.read(cx).config.clone())
            .collect())
    }
    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match sofia_config::import_mcp_servers(self.json.read(cx).value().as_ref()) {
            Err(error) => self.status = error,
            Ok(configs) => {
                let count = configs.len();
                for mut config in configs {
                    if let Some(index) = self
                        .servers
                        .iter()
                        .position(|server| server.read(cx).config.id == config.id)
                    {
                        let previous = &self.servers[index].read(cx).config;
                        config.enabled = previous.enabled;
                        config.disabled_tools = previous.disabled_tools.clone();
                        if let (
                            McpTransport::Http {
                                oauth: Some(next), ..
                            },
                            McpTransport::Http {
                                oauth: Some(previous),
                                ..
                            },
                        ) = (&mut config.transport, &previous.transport)
                            && next.client_id == previous.client_id
                        {
                            next.token = previous.token.clone();
                            next.token_received_at = previous.token_received_at;
                        }
                        self.servers[index] = cx.new(|cx| ServerCard::new(config, cx));
                    } else {
                        self.servers.push(cx.new(|cx| ServerCard::new(config, cx)));
                    }
                }
                self.status =
                    format!("Added or updated {count} servers. Save & apply to connect Sofia.");
                self.json
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
        }
        cx.notify();
    }
}
impl ServerCard {
    fn new(config: McpServerConfig, cx: &mut Context<Self>) -> Self {
        let (sender, results) = mpsc::channel();
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        if let Ok(result) = view.results.try_recv() {
                            view.pending = false;
                            match result {
                                CardResult::Tools(Ok(tools)) => {
                                    view.status = format!("{} tools available", tools.len());
                                    view.tools = tools;
                                }
                                CardResult::Tools(Err(error)) | CardResult::OAuth(Err(error)) => {
                                    view.status = error
                                }
                                CardResult::OAuth(Ok(oauth)) => {
                                    if let McpTransport::Http {
                                        oauth: current @ Some(_),
                                        ..
                                    } = &mut view.config.transport
                                    {
                                        *current = Some(oauth);
                                    }
                                    view.status =
                                        match sofia_config::upsert_mcp_server(&view.config) {
                                            Ok(()) => {
                                                "Authenticated · restart Sofia to connect".into()
                                            }
                                            Err(error) => format!(
                                                "Authenticated, but could not save: {error}"
                                            ),
                                        };
                                }
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            config,
            tools: vec![],
            status: "Ready to test".into(),
            pending: false,
            results,
            sender,
        }
    }
    fn test(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let mut config = self.config.clone();
        config.enabled = true;
        self.pending = true;
        self.status = "Connecting…".into();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Cannot start MCP test runtime".into())
                .and_then(|runtime| {
                    runtime.block_on(async {
                        sofia_mcp_client::ConnectedServer::connect(&config)
                            .await
                            .map(|server| {
                                server
                                    .tools
                                    .iter()
                                    .map(|tool| tool.name.to_string())
                                    .collect()
                            })
                    })
                });
            let _ = sender.send(CardResult::Tools(result));
        });
        cx.notify();
    }
    fn authenticate(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.status = "Opening browser…".into();
        let config = self.config.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Cannot start OAuth runtime".into())
                .and_then(|runtime| {
                    runtime.block_on(sofia_mcp_client::authenticate_oauth(&config))
                });
            let _ = sender.send(CardResult::OAuth(result));
        });
        cx.notify();
    }
}
impl Render for ServerCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.config.name.is_empty() {
            self.config.id.clone()
        } else {
            self.config.name.clone()
        };
        let transport = match &self.config.transport {
            McpTransport::Stdio { command, .. } => format!("Local · {command}"),
            McpTransport::Http {
                oauth: Some(oauth), ..
            } if oauth.token.is_some() => "Remote · OAuth connected".into(),
            McpTransport::Http { oauth: Some(_), .. } => "Remote · OAuth required".into(),
            McpTransport::Http { .. } => "Remote · HTTP".into(),
        };
        let uses_oauth = matches!(
            self.config.transport,
            McpTransport::Http { oauth: Some(_), .. }
        );
        let mut content = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(title)
                            .child(div().text_sm().child(transport)),
                    )
                    .child(
                        Switch::new("enabled")
                            .label("Enabled")
                            .checked(self.config.enabled)
                            .on_click(cx.listener(|view, value: &bool, _, cx| {
                                view.config.enabled = *value;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("test")
                            .label(if self.pending {
                                "Connecting…"
                            } else {
                                "Test connection"
                            })
                            .on_click(cx.listener(|view, _, _, cx| view.test(cx))),
                    )
                    .child(div().text_sm().child(self.status.clone())),
            );
        if uses_oauth {
            content = content.child(
                Button::new("authenticate")
                    .label("Authenticate")
                    .on_click(cx.listener(|view, _, _, cx| view.authenticate(cx))),
            );
        }
        for name in &self.tools {
            let tool = name.clone();
            content = content.child(
                Switch::new(SharedString::from(name.clone()))
                    .label(name.clone())
                    .checked(!self.config.disabled_tools.contains(name))
                    .on_click(cx.listener(move |view, enabled: &bool, _, cx| {
                        view.config.disabled_tools.retain(|name| name != &tool);
                        if !enabled {
                            view.config.disabled_tools.push(tool.clone());
                        }
                        cx.notify();
                    })),
            );
        }
        content
    }
}
impl Render for McpPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .w_full()
            .max_w(rems(48.))
            .flex()
            .flex_col()
            .gap_4()
            .child("Paste MCP JSON")
            .child(Textarea::new(&self.json).w_full())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("import-servers")
                            .label("Add servers")
                            .on_click(cx.listener(|view, _, window, cx| view.import(window, cx))),
                    )
                    .child(div().text_sm().child(self.status.clone())),
            );
        for (index, server) in self.servers.iter().enumerate() {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .py_4()
                    .child(server.clone())
                    .child(
                        div().flex().child(
                            Button::new(("remove-server", index))
                                .label("Remove")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.servers.remove(index);
                                    cx.notify();
                                })),
                        ),
                    ),
            );
        }
        if self.servers.is_empty() {
            content = content.child("No MCP servers added yet.");
        }
        content
    }
}
