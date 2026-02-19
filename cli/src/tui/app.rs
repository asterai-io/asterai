use std::path::PathBuf;

pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// (display_name, env_var_key, &[(model_id, model_label)]).
pub type Provider = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

pub const PROVIDERS: &[Provider] = &[
    (
        "Anthropic (Claude)",
        "ANTHROPIC_KEY",
        &[
            (
                "anthropic/claude-sonnet-4-6",
                "Claude Sonnet 4.6 (recommended)",
            ),
            ("anthropic/claude-opus-4-6", "Claude Opus 4.6"),
            ("anthropic/claude-haiku-4-5", "Claude Haiku 4.5"),
        ],
    ),
    (
        "OpenAI (GPT)",
        "OPENAI_KEY",
        &[
            ("openai/gpt-5-mini", "GPT-5 Mini (recommended)"),
            ("openai/gpt-5.2", "GPT-5.2"),
            ("openai/gpt-5-nano", "GPT-5 Nano"),
        ],
    ),
    (
        "Google (Gemini)",
        "GOOGLE_KEY",
        &[
            ("google/gemini-3-flash-preview", "Gemini 3 Flash Preview (recommended)"),
            ("google/gemini-2.5-flash", "Gemini 2.5 Flash"),
            ("google/gemini-2.5-pro", "Gemini 2.5 Pro"),
        ],
    ),
    (
        "Venice",
        "VENICE_KEY",
        &[
            ("venice/kimi-k2-5", "Kimi K2.5 (recommended)"),
            ("venice/zai-org-glm-5", "GLM 5"),
            ("venice/venice-uncensored", "Venice Uncensored 1.1"),
        ],
    ),
];

pub const CORE_COMPONENTS: &[&str] = &[
    "asterbot:agent",
    "asterbot:core",
    "asterbot:toolkit",
    "asterai:llm",
];

pub const DEFAULT_TOOLS: &[&str] = &["asterbot:soul", "asterbot:memory", "asterbot:skills"];

pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "help",
        description: "Show available commands",
    },
    SlashCommand {
        name: "tools",
        description: "List, add, or remove tools",
    },
    SlashCommand {
        name: "clear",
        description: "Clear conversation history",
    },
    SlashCommand {
        name: "model",
        description: "View or switch LLM model",
    },
    SlashCommand {
        name: "name",
        description: "View or change agent name",
    },
    SlashCommand {
        name: "dir",
        description: "Manage directory access",
    },
    SlashCommand {
        name: "status",
        description: "Show agent status",
    },
    SlashCommand {
        name: "config",
        description: "Manage env variables",
    },
    SlashCommand {
        name: "push",
        description: "Push agent to cloud",
    },
    SlashCommand {
        name: "pull",
        description: "Pull agent from cloud",
    },
];

pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
}

/// Which screen is active.
pub enum Screen {
    Auth(AuthState),
    Picker(PickerState),
    Setup(SetupState),
    Chat(ChatState),
}

pub enum AuthState {
    Checking,
    NeedLogin {
        input: String,
        error: Option<String>,
    },
    LoggingIn,
}

pub enum SetupStep {
    Name,
    Provider,
    ApiKey,
    Model,
    Directories,
    Provisioning {
        current: usize,
        total: usize,
        message: String,
    },
    WarmUp,
    PushPrompt,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// Runtime configuration for the active agent. Built from environment data.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub env_name: String,
    pub bot_name: String,
    pub model: Option<String>,
    pub provider: String,
    pub tools: Vec<String>,
    pub allowed_dirs: Vec<String>,
}

#[derive(Clone)]
pub struct AgentEntry {
    pub name: String,
    pub namespace: String,
    pub component_count: usize,
    pub bot_name: String,
    pub model: Option<String>,
    pub is_remote: bool,
}

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub agent: Option<AgentConfig>,
    pub pending_response: Option<tokio::sync::oneshot::Receiver<eyre::Result<Option<String>>>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::Auth(AuthState::Checking),
            should_quit: false,
            agent: None,
            pending_response: None,
        }
    }
}

impl App {
    pub fn push_message(&mut self, role: MessageRole, content: String) {
        if let Screen::Chat(state) = &mut self.screen {
            state.messages.push(ChatMessage { role, content });
        }
    }
}

pub struct PickerState {
    pub agents: Vec<AgentEntry>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

pub struct SetupState {
    pub step: SetupStep,
    pub bot_name: String,
    pub env_name: String,
    pub provider_idx: usize,
    pub api_key: String,
    pub model: String,
    pub model_idx: usize,
    pub allowed_dirs: Vec<String>,
    pub input: String,
    pub error: Option<String>,
}

impl Default for SetupState {
    fn default() -> Self {
        Self {
            step: SetupStep::Name,
            bot_name: String::new(),
            env_name: String::new(),
            provider_idx: 0,
            api_key: String::new(),
            model: String::new(),
            model_idx: 0,
            allowed_dirs: Vec::new(),
            input: String::new(),
            error: None,
        }
    }
}

pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Default)]
pub struct ChatState {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    pub waiting: bool,
    pub spinner_tick: usize,
    pub slash_matches: Vec<usize>,
    pub slash_selected: usize,
    pub scroll_offset: u16,
}

/// Resolve the state directory for an agent.
pub fn resolve_state_dir(env_name: &str) -> PathBuf {
    crate::config::BASE_DIR.join("agents").join(env_name)
}

/// Sanitize a display name to a valid environment name.
pub fn sanitize_bot_name(display_name: &str) -> String {
    let sanitized: String = display_name
        .to_lowercase()
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() || c == '-' {
            true => c,
            false => '-',
        })
        .collect();
    let trimmed = sanitized.trim_matches('-').replace("--", "-");
    let end = trimmed.len().min(40);
    let result = &trimmed[..end];
    match result.is_empty() {
        true => "asterbot".to_string(),
        false => result.to_string(),
    }
}
