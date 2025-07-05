use crate::tauri_api::{
    configure_tlsforward, enable_tlsforward, get_daemon_runtime_config, get_daemon_status,
    start_daemon, DaemonRuntimeConfig, TlsForwardState,
};
use gloo_timers::callback::Interval;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::InputEvent;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct DaemonStatusProps {
    #[prop_or(true)]
    pub is_dark: bool,
}

pub struct DaemonStatusComponent {
    is_running: bool,
    listen_address: Option<String>,
    has_upstreams: bool,
    runtime_config: Option<DaemonRuntimeConfig>,
    error_message: Option<String>,
    debug_messages: Vec<String>,
    _poll_interval: Option<Interval>,
    show_tlsforward_form: bool,
    tlsforward_email: String,
    tlsforward_loading: bool,
    initial_connect_attempts: u32,
    poll_delay_ms: u32,
    show_debug_log: bool,
}

pub enum Msg {
    UpdateStatus(bool, Option<String>, bool),
    UpdateConfig(DaemonRuntimeConfig),
    Refresh,
    StartDaemon,
    SetError(String),
    AddDebugMessage(String),
    ShowTlsForwardForm,
    HideTlsForwardForm,
    UpdateTlsForwardEmail(String),
    ConfigureTlsForward,
    TlsForwardConfigured,
    ToggleDebugLog,
    OpenUrl(String),
}

impl Component for DaemonStatusComponent {
    type Message = Msg;
    type Properties = DaemonStatusProps;

    fn create(ctx: &Context<Self>) -> Self {
        // Add initial debug message about window location
        ctx.link().send_message(Msg::AddDebugMessage(format!(
            "Window location: {}",
            web_sys::window()
                .unwrap()
                .location()
                .href()
                .unwrap_or_default()
        )));

        // Start with faster polling for initial connection
        let component = Self {
            is_running: false,
            listen_address: None,
            has_upstreams: false,
            runtime_config: None,
            error_message: None,
            debug_messages: vec![],
            _poll_interval: None,
            show_tlsforward_form: false,
            tlsforward_email: String::new(),
            tlsforward_loading: false,
            initial_connect_attempts: 0,
            poll_delay_ms: 1, // Start with 1ms polling
            show_debug_log: false,
        };

        // Fetch initial status immediately
        ctx.link().send_message(Msg::Refresh);

        component
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::UpdateStatus(running, address, upstreams) => {
                self.is_running = running;
                self.listen_address = address;
                self.has_upstreams = upstreams;
                true
            }
            Msg::UpdateConfig(config) => {
                self.runtime_config = Some(config);
                true
            }
            Msg::Refresh => {
                let link = ctx.link().clone();

                spawn_local(async move {
                    // Get daemon status
                    match get_daemon_status().await {
                        Ok(status) => {
                            link.send_message(Msg::UpdateStatus(
                                status.running,
                                status.listen_address,
                                status.has_upstreams,
                            ));

                            // If running, get config too
                            if status.running {
                                match get_daemon_runtime_config().await {
                                    Ok(config) => {
                                        link.send_message(Msg::UpdateConfig(config));
                                    }
                                    Err(_) => {
                                        // Silent error during polling
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            // Silent error during polling - daemon might be starting
                        }
                    }
                });

                // Schedule next poll with exponential backoff
                if !self.is_running {
                    self.initial_connect_attempts += 1;

                    // Exponential backoff: 1ms -> 5ms -> 25ms -> 125ms -> 500ms -> 2000ms
                    if self.poll_delay_ms < 2000 {
                        self.poll_delay_ms = match self.poll_delay_ms {
                            1 => 5,
                            5 => 25,
                            25 => 125,
                            125 => 200,
                            200 => 500,
                            _ => 500,
                        };
                    }
                } else {
                    self.poll_delay_ms = 500;
                }

                // Set up next poll
                self._poll_interval = None;
                let link = ctx.link().clone();
                let interval = Interval::new(self.poll_delay_ms, move || {
                    link.send_message(Msg::Refresh);
                });
                self._poll_interval = Some(interval);

                false
            }
            Msg::StartDaemon => {
                web_sys::console::log_1(&"Starting daemon...".into());
                ctx.link()
                    .send_message(Msg::AddDebugMessage("Starting daemon...".to_string()));
                let link = ctx.link().clone();
                spawn_local(async move {
                    // Start daemon with default config
                    match start_daemon(None).await {
                        Ok(msg) => {
                            web_sys::console::log_1(&format!("Daemon started: {msg}").into());
                            link.send_message(Msg::AddDebugMessage(format!(
                                "Daemon started: {msg}"
                            )));
                            // Refresh status after starting
                            link.send_message(Msg::Refresh);
                        }
                        Err(e) => {
                            web_sys::console::error_1(
                                &format!("Failed to start daemon: {e}").into(),
                            );
                            link.send_message(Msg::SetError(format!(
                                "Failed to start daemon: {e}"
                            )));
                        }
                    }
                });
                false
            }
            Msg::SetError(error) => {
                self.error_message = Some(error);
                true
            }
            Msg::AddDebugMessage(msg) => {
                self.debug_messages.push(msg);
                if self.debug_messages.len() > 10 {
                    self.debug_messages.remove(0);
                }
                true
            }
            Msg::ShowTlsForwardForm => {
                self.show_tlsforward_form = true;
                true
            }
            Msg::HideTlsForwardForm => {
                self.show_tlsforward_form = false;
                self.tlsforward_email.clear();
                true
            }
            Msg::UpdateTlsForwardEmail(email) => {
                self.tlsforward_email = email;
                true
            }
            Msg::ConfigureTlsForward => {
                if self.tlsforward_email.contains('@') && self.tlsforward_email.len() > 3 {
                    self.tlsforward_loading = true;
                    let email = self.tlsforward_email.clone();
                    let link = ctx.link().clone();
                    spawn_local(async move {
                        match configure_tlsforward(email).await {
                            Ok(_) => {
                                match enable_tlsforward().await {
                                    Ok(_) => {
                                        link.send_message(Msg::TlsForwardConfigured);
                                        link.send_message(Msg::HideTlsForwardForm);
                                        // Refresh to get new status
                                        link.send_message(Msg::Refresh);
                                    }
                                    Err(e) => {
                                        link.send_message(Msg::SetError(format!(
                                            "Failed to enable TLS forward: {e}"
                                        )));
                                    }
                                }
                            }
                            Err(e) => {
                                link.send_message(Msg::SetError(format!(
                                    "Failed to configure: {e}"
                                )));
                            }
                        }
                    });
                } else {
                    self.error_message = Some("Please enter a valid email address".to_string());
                }
                true
            }
            Msg::TlsForwardConfigured => {
                self.tlsforward_loading = false;
                true
            }
            Msg::ToggleDebugLog => {
                web_sys::console::log_1(
                    &format!(
                        "Debug button clicked! Current state: {}, New state: {}",
                        self.show_debug_log, !self.show_debug_log
                    )
                    .into(),
                );
                self.show_debug_log = !self.show_debug_log;
                ctx.link().send_message(Msg::AddDebugMessage(format!(
                    "Debug log toggled to: {}",
                    self.show_debug_log
                )));
                true
            }
            Msg::OpenUrl(url) => {
                web_sys::console::log_1(&format!("OpenUrl message received for: {url}").into());
                ctx.link().send_message(Msg::AddDebugMessage(format!(
                    "Attempting to open URL: {url}"
                )));
                let link = ctx.link().clone();
                spawn_local(async move {
                    web_sys::console::log_1(
                        &format!("Starting async open_url call for: {url}").into(),
                    );
                    match crate::tauri_api::open_url(url.clone()).await {
                        Ok(_) => {
                            web_sys::console::log_1(
                                &format!("Successfully opened URL: {url}").into(),
                            );
                            link.send_message(Msg::AddDebugMessage(format!("✓ Opened URL: {url}")));
                        }
                        Err(e) => {
                            web_sys::console::error_1(
                                &format!("Failed to open URL {url}: {e}").into(),
                            );
                            link.send_message(Msg::SetError(format!("Failed to open URL: {e}")));
                            link.send_message(Msg::AddDebugMessage(format!(
                                "✗ Failed to open URL: {url} - Error: {e}"
                            )));
                        }
                    }
                });
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let is_dark = ctx.props().is_dark;
        html! {
            <div class="w-full max-w-2xl mx-auto">
                <div class="mb-8">
                    <h1 class={classes!("text-2xl", "font-semibold", "mb-2", if is_dark { "text-white" } else { "text-gray-900" })}>{"Gate AI Gateway"}</h1>
                    <p class={classes!("text-sm", if is_dark { "text-gray-400" } else { "text-gray-600" })}>{"Local inference daemon status and configuration"}</p>
                </div>

                <div class="space-y-6">
                    <div>
                        <h3 class={classes!("text-sm", "font-medium", "mb-3", "uppercase", "tracking-wider", if is_dark { "text-gray-400" } else { "text-gray-500" })}>{"Connection Status"}</h3>
                        <button
                            onclick={ctx.link().callback(|e: MouseEvent| {
                                web_sys::console::log_1(&"Debug button clicked (from onclick)".into());
                                e.prevent_default();
                                e.stop_propagation();
                                Msg::ToggleDebugLog
                            })}
                            class={classes!(
                                "border", "rounded", "px-3", "py-1", "text-sm", "cursor-pointer", "transition-all",
                                if self.show_debug_log {
                                    if is_dark { "bg-gray-700 border-gray-600 text-gray-300 hover:bg-gray-600" } else { "bg-gray-200 border-gray-300 text-gray-700 hover:bg-gray-300" }
                                } else if is_dark { "bg-gray-800 border-gray-700 text-gray-400 hover:bg-gray-700" } else { "bg-white border-gray-300 text-gray-600 hover:bg-gray-100" }
                            )}
                            title={if self.show_debug_log { "Hide debug log" } else { "Show debug log" }}
                        >
                            {if self.show_debug_log { "🐛 Debug ON" } else { "🐛 Debug" }}
                        </button>
                    </div>

                    <div class="space-y-2">
                        <div class="flex items-center justify-between">
                            <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"Daemon Status"}</span>
                            <span class={classes!(
                                "px-2", "py-1", "rounded", "text-xs", "font-medium", "uppercase", "tracking-wider",
                                if self.is_running {
                                    if is_dark { "text-green-400 bg-green-900/50" } else { "text-green-700 bg-green-50" }
                                } else if is_dark { "text-red-400 bg-red-900/50" } else { "text-red-700 bg-red-50" }
                            )}>
                                {if self.is_running {
                                    "Connected"
                                } else if self.poll_delay_ms < 500 {
                                    "Starting..."
                                } else {
                                    "Disconnected"
                                }}
                            </span>
                        </div>

                        {if let Some(addr) = &self.listen_address {
                            html! {
                                <div class="flex items-center justify-between">
                                    <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"Listen Address"}</span>
                                    <span class={classes!("text-sm", "font-mono", if is_dark { "text-gray-200" } else { "text-gray-800" })}>{addr}</span>
                                </div>
                            }
                        } else {
                            html! {}
                        }}

                        {if let Some(config) = &self.runtime_config {
                            html! {
                                <>
                                    <div class="flex items-center justify-between">
                                        <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"Database"}</span>
                                        <span class={classes!("text-sm", "font-mono", if is_dark { "text-gray-200" } else { "text-gray-800" })}>
                                            {if config.database_url.contains(":memory:") {
                                                "In-memory"
                                            } else if config.database_url.starts_with("sqlite://") {
                                                "Persistent"
                                            } else {
                                                &config.database_url
                                            }}
                                        </span>
                                    </div>

                                    <div class="flex items-center justify-between">
                                        <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"Upstreams"}</span>
                                        <span class={classes!("text-sm", if is_dark { "text-gray-200" } else { "text-gray-800" })}>
                                            {if config.upstream_count > 0 {
                                                format!("{} configured", config.upstream_count)
                                            } else {
                                                "None configured".to_string()
                                            }}
                                        </span>
                                    </div>

                                    {if let Some(node_id) = &config.p2p_node_id {
                                        let short_id = if node_id.len() > 16 {
                                            format!("{}...{}", &node_id[..8], &node_id[node_id.len()-4..])
                                        } else {
                                            node_id.clone()
                                        };
                                        html! {
                                            <div class="flex items-center justify-between">
                                                <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"P2P Node ID"}</span>
                                                <span
                                                    class={classes!("text-xs", "font-mono", "cursor-pointer", if is_dark { "text-gray-200 hover:text-gray-400" } else { "text-gray-800 hover:text-gray-600" })}
                                                    title={node_id.clone()}
                                                    onclick={
                                                        let node_id = node_id.clone();
                                                        Callback::from(move |_| {
                                                            if let Some(window) = web_sys::window() {
                                                                let _ = window.navigator().clipboard().write_text(&node_id);
                                                            }
                                                        })
                                                    }
                                                >
                                                    {short_id}
                                                </span>
                                            </div>
                                        }
                                    } else {
                                        html! {}
                                    }}

                                    {if !config.p2p_listen_addresses.is_empty() {
                                        // Group addresses by type (IPv4 and IPv6)
                                        let addresses = config.p2p_listen_addresses.join(" ");
                                        html! {
                                            <div class="flex items-center justify-between">
                                                <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"P2P Listen"}</span>
                                                <span class={classes!("text-xs", "font-mono", "max-w-[200px]", "overflow-hidden", "text-ellipsis", "whitespace-nowrap", "text-right", if is_dark { "text-gray-200" } else { "text-gray-800" })} title={addresses.clone()}>
                                                    {addresses}
                                                </span>
                                            </div>
                                        }
                                    } else {
                                        html! {}
                                    }}

                                    {if config.tlsforward_enabled || config.tlsforward_state.is_some() {
                                        html! {
                                            <div class="flex items-center justify-between">
                                                <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"TLS Forward"}</span>
                                                <span class={classes!("text-sm", if is_dark { "text-gray-200" } else { "text-gray-800" })}>
                                                    {match &config.tlsforward_state {
                                                        Some(TlsForwardState::Disabled) => "Disabled",
                                                        Some(TlsForwardState::Disconnected) => "Disconnected",
                                                        Some(TlsForwardState::Connecting) => "Requesting certificate...",
                                                        Some(TlsForwardState::Connected { .. }) => "Connected",
                                                        Some(TlsForwardState::Error(_)) => "Error",
                                                        None => "Not configured",
                                                    }}
                                                </span>
                                            </div>
                                        }
                                    } else if !self.show_tlsforward_form {
                                        html! {
                                            <div class="flex items-center justify-between">
                                                <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"TLS Forward"}</span>
                                                <button
                                                    onclick={ctx.link().callback(|_| Msg::ShowTlsForwardForm)}
                                                    class={classes!("text-xs", "font-medium", "border", "rounded", "px-3", "py-1", "cursor-pointer", "transition-colors", if is_dark { "bg-gray-800 hover:bg-gray-700 border-gray-700 text-gray-300" } else { "bg-white hover:bg-gray-50 border-gray-300 text-gray-700" })}
                                                >
                                                    {"Configure"}
                                                </button>
                                            </div>
                                        }
                                    } else {
                                        html! {}
                                    }}

                                    {match &config.tlsforward_state {
                                        Some(TlsForwardState::Connected { server_address: _, assigned_domain }) => {
                                            html! {
                                                <div class="flex items-center justify-between">
                                                    <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"Encrypted URL"}</span>
                                                    <div class="flex items-center gap-2">
                                                        <a
                                                            href={format!("https://{assigned_domain}")}
                                                            class={classes!("text-xs", "font-mono", "font-medium", "underline", if is_dark { "text-green-400 hover:text-green-300" } else { "text-green-600 hover:text-green-700" })}
                                                            title="Click to open in browser"
                                                            onclick={
                                                                let full_url = format!("https://{assigned_domain}");
                                                                let link = ctx.link().clone();
                                                                Callback::from(move |e: MouseEvent| {
                                                                    web_sys::console::log_1(&format!("Link clicked: {full_url}").into());
                                                                    e.prevent_default();
                                                                    e.stop_propagation();
                                                                    let url = full_url.clone();
                                                                    link.send_message(Msg::OpenUrl(url));
                                                                })
                                                            }
                                                        >
                                                            {format!("https://{assigned_domain}")}
                                                        </a>
                                                        <button
                                                            class={classes!("p-1", if is_dark { "text-gray-400 hover:text-gray-200" } else { "text-gray-400 hover:text-gray-600" })}
                                                            title="Copy to clipboard"
                                                            onclick={
                                                                let full_url = format!("https://{assigned_domain}");
                                                                Callback::from(move |_| {
                                                                    if let Some(window) = web_sys::window() {
                                                                        let _ = window.navigator().clipboard().write_text(&full_url);
                                                                    }
                                                                })
                                                            }
                                                        >
                                                            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"></path>
                                                            </svg>
                                                        </button>
                                                    </div>
                                                </div>
                                            }
                                        }
                                        Some(TlsForwardState::Error(msg)) => {
                                            html! {
                                                <div class="flex items-start justify-between">
                                                    <span class={classes!("text-xs", "uppercase", "tracking-wider", "font-medium", "text-gray-500")}>{"Status"}</span>
                                                    <span class={classes!("text-xs", "text-right", "max-w-[200px]", if is_dark { "text-gray-400" } else { "text-gray-500" })}>
                                                        {msg}
                                                    </span>
                                                </div>
                                            }
                                        }
                                        _ => html! {}
                                    }}
                                </>
                            }
                        } else {
                            html! {}
                        }}
                    </div>
                </div>

                {if self.show_tlsforward_form && self.is_running {
                    html! {
                        <div class="mt-6">
                            <h4 class={classes!("text-sm", "font-medium", "mb-3", "uppercase", "tracking-wider", if is_dark { "text-gray-400" } else { "text-gray-500" })}>{"Configure TLS Forwarding"}</h4>

                            <p class={classes!("text-sm", "m-0", "mb-4", if is_dark { "text-gray-300" } else { "text-gray-500" })}>
                                {"TLS forwarding allows you to access your Gate instance via HTTPS with automatic SSL certificates."}
                            </p>

                            <div class="mb-4">
                                <label class={classes!("block", "text-sm", "font-medium", "mb-2", if is_dark { "text-gray-200" } else { "text-gray-700" })}>{"Email for Let's Encrypt (required)"}</label>
                                <input
                                    type="email"
                                    value={self.tlsforward_email.clone()}
                                    oninput={ctx.link().callback(|e: InputEvent| {
                                        let input = e.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                                        Msg::UpdateTlsForwardEmail(input.value())
                                    })}
                                    placeholder="your@email.com"
                                    class={classes!("w-full", "py-2", "px-3", "border", "rounded-md", "text-sm", "focus:outline-none", "focus:ring-2", if is_dark { "border-white/20 bg-white/10 text-white placeholder-gray-400 focus:ring-blue-400 focus:border-blue-400 backdrop-blur-sm" } else { "border-gray-300 bg-white text-gray-800 placeholder-gray-400 focus:ring-blue-500 focus:border-blue-500" })}
                                    disabled={self.tlsforward_loading}
                                />
                            </div>

                            <div class="flex gap-2">
                                <button
                                    onclick={ctx.link().callback(|_| Msg::ConfigureTlsForward)}
                                    disabled={self.tlsforward_loading || self.tlsforward_email.is_empty()}
                                    class={classes!(
                                        "flex-1", "text-white", "border-none", "rounded-md", "py-2", "px-4", "text-sm", "font-medium",
                                        if self.tlsforward_loading || self.tlsforward_email.is_empty() {
                                            if is_dark { "bg-gray-800 border-gray-700 text-gray-600 cursor-not-allowed" } else { "bg-gray-100 border-gray-300 text-gray-400 cursor-not-allowed" }
                                        } else {
                                            "bg-green-600 hover:bg-green-700 text-white cursor-pointer"
                                        }
                                    )}
                                >
                                    {if self.tlsforward_loading { "Configuring..." } else { "Enable TLS Forwarding" }}
                                </button>
                                <button
                                    onclick={ctx.link().callback(|_| Msg::HideTlsForwardForm)}
                                    disabled={self.tlsforward_loading}
                                    class={classes!("border", "rounded-md", "py-2", "px-4", "text-sm", "font-medium", "cursor-pointer", "transition-colors", if is_dark { "bg-transparent text-gray-400 border-gray-700 hover:bg-gray-800" } else { "bg-white text-gray-700 border-gray-300 hover:bg-gray-50" })}
                                >
                                    {"Cancel"}
                                </button>
                            </div>
                        </div>
                    }
                } else {
                    html! {}
                }}

                {if !self.is_running {
                    html! {
                        <div class="mt-4">
                            <button
                                onclick={ctx.link().callback(|_| Msg::StartDaemon)}
                                class={classes!("w-full", "text-white", "border-none", "rounded", "py-2.5", "px-4", "text-sm", "font-medium", "cursor-pointer", "transition-colors", "duration-200", "bg-green-600", "hover:bg-green-700")}
                            >
                                {"Start Daemon"}
                            </button>
                        </div>
                    }
                } else {
                    html! {}
                }}

                {if let Some(error) = &self.error_message {
                    html! {
                        <div class={classes!("mt-4", "p-3", "border", "rounded", "text-sm", if is_dark { "bg-red-900/20 border-red-800 text-red-400" } else { "bg-red-50 border-red-200 text-red-700" })}>
                            <strong>{"Error: "}</strong>{error}
                        </div>
                    }
                } else {
                    html! {}
                }}

                {if self.show_debug_log && !self.debug_messages.is_empty() {
                    html! {
                        <div class={classes!("mt-4", "p-3", "border", "rounded", "text-xs", "font-mono", if is_dark { "bg-gray-800 border-gray-700 text-gray-400" } else { "bg-gray-50 border-gray-300 text-gray-700" })}>
                            <div class="font-semibold mb-2">{"Debug Log:"}</div>
                            {for self.debug_messages.iter().map(|msg| {
                                html! { <div>{msg}</div> }
                            })}
                        </div>
                    }
                } else {
                    html! {}
                }}
            </div>
        }
    }
}
