use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub const QUOTES: &[&str] = &[
    "\"The only way to do great work is to love what you do.\" — Steve Jobs",
    "\"The best way to predict the future is to invent it.\" — Alan Kay",
    "\"Any sufficiently advanced technology is indistinguishable from magic.\" — Arthur C. Clarke",
    "\"Shall we play a game?\" — WOPR, WarGames",
    "\"I'm sorry, Dave. I'm afraid I can't do that.\" — HAL 9000",
    "\"The question of whether a computer can think is no more interesting than whether a submarine can swim.\" — Dijkstra",
    "\"Talk is cheap. Show me the code.\" — Linus Torvalds",
    "\"Programs must be written for people to read.\" — Abelson & Sussman",
    "\"First, solve the problem. Then, write the code.\" — John Johnson",
    "\"Simplicity is the ultimate sophistication.\" — Leonardo da Vinci",
    "\"It's not a bug, it's a feature.\" — Anonymous",
    "\"There are only two hard things: cache invalidation and naming things.\" — Phil Karlton",
    "\"The computer was born to solve problems that did not exist before.\" — Bill Gates",
    "\"Machines take me by surprise with great frequency.\" — Alan Turing",
    "\"We can only see a short distance ahead, but we can see plenty there that needs to be done.\" — Alan Turing",
    "\"A computer once beat me at chess, but it was no match for me at kickboxing.\" — Emo Philips",
    "\"The Net is a waste of time, and that's exactly what's right about it.\" — William Gibson",
    "\"The future is already here - it's just not evenly distributed.\" — William Gibson",
    "\"Never trust a computer you can't throw out a window.\" — Steve Wozniak",
    "\"People who are really serious about software should make their own hardware.\" — Alan Kay",
    "\"In the beginning the Universe was created. This made a lot of people angry.\" — Douglas Adams",
    "\"Don't panic.\" — The Hitchhiker's Guide to the Galaxy",
    "\"I think, therefore I am.\" — Rene Descartes",
    "\"The only true wisdom is in knowing you know nothing.\" — Socrates",
    "\"Information wants to be free.\" — Stewart Brand",
    "\"Move fast and break things.\" — Mark Zuckerberg",
    "\"Stay hungry. Stay foolish.\" — Stewart Brand / Steve Jobs",
    "\"The medium is the message.\" — Marshall McLuhan",
    "\"We shape our tools, and thereafter our tools shape us.\" — McLuhan",
    "\"Reality is merely an illusion, albeit a very persistent one.\" — Albert Einstein",
];

pub fn random_quote() -> &'static str {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    QUOTES[nanos % QUOTES.len()]
}

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
            (
                "google/gemini-3-flash-preview",
                "Gemini 3 Flash Preview (recommended)",
            ),
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
        subs: &[],
    },
    SlashCommand {
        name: "tools",
        description: "List, add, or remove tools",
        subs: &[
            SubCommand {
                name: "list",
                description: "Show installed tools",
                needs_arg: false,
            },
            SubCommand {
                name: "add",
                description: "Add a tool component",
                needs_arg: true,
            },
            SubCommand {
                name: "remove",
                description: "Remove a tool",
                needs_arg: true,
            },
        ],
    },
    SlashCommand {
        name: "clear",
        description: "Clear conversation history",
        subs: &[],
    },
    SlashCommand {
        name: "model",
        description: "View or switch LLM model",
        subs: &[],
    },
    SlashCommand {
        name: "name",
        description: "View or change agent name",
        subs: &[],
    },
    SlashCommand {
        name: "username",
        description: "View or change your display name",
        subs: &[],
    },
    SlashCommand {
        name: "dir",
        description: "Manage directory access",
        subs: &[
            SubCommand {
                name: "list",
                description: "Show allowed directories",
                needs_arg: false,
            },
            SubCommand {
                name: "add",
                description: "Grant directory access",
                needs_arg: true,
            },
            SubCommand {
                name: "remove",
                description: "Revoke directory access",
                needs_arg: true,
            },
        ],
    },
    SlashCommand {
        name: "status",
        description: "Show agent status",
        subs: &[],
    },
    SlashCommand {
        name: "config",
        description: "Manage env variables",
        subs: &[
            SubCommand {
                name: "list",
                description: "Show all variables",
                needs_arg: false,
            },
            SubCommand {
                name: "set",
                description: "Set KEY=VALUE",
                needs_arg: true,
            },
        ],
    },
    SlashCommand {
        name: "banner",
        description: "Configure banner content",
        subs: &[
            SubCommand {
                name: "auto",
                description: "Agent picks content from tools",
                needs_arg: false,
            },
            SubCommand {
                name: "quote",
                description: "Random quotes only",
                needs_arg: false,
            },
            SubCommand {
                name: "off",
                description: "No banner content",
                needs_arg: false,
            },
        ],
    },
    SlashCommand {
        name: "push",
        description: "Push agent to cloud",
        subs: &[],
    },
    SlashCommand {
        name: "pull",
        description: "Pull agent from cloud",
        subs: &[],
    },
];

pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub subs: &'static [SubCommand],
}

pub struct SubCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub needs_arg: bool,
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
    Username,
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
    pub user_name: String,
    pub model: Option<String>,
    pub provider: String,
    pub tools: Vec<String>,
    pub allowed_dirs: Vec<String>,
    /// Banner mode: "auto", "quote", or "off".
    pub banner_mode: String,
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
    pub pending_banner: Option<tokio::sync::oneshot::Receiver<Option<String>>>,
    pub pending_warmup: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::Auth(AuthState::Checking),
            should_quit: false,
            agent: None,
            pending_response: None,
            pending_banner: None,
            pending_warmup: None,
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
    pub user_name: String,
    pub provider_idx: usize,
    pub api_key: String,
    pub model: String,
    pub model_idx: usize,
    pub allowed_dirs: Vec<String>,
    pub input: String,
    pub error: Option<String>,
    pub spinner_tick: usize,
}

impl Default for SetupState {
    fn default() -> Self {
        Self {
            step: SetupStep::Name,
            bot_name: String::new(),
            env_name: String::new(),
            user_name: String::new(),
            provider_idx: 0,
            api_key: String::new(),
            model: String::new(),
            model_idx: 0,
            allowed_dirs: Vec::new(),
            input: String::new(),
            error: None,
            spinner_tick: 0,
        }
    }
}

pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

pub struct ChatState {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    pub waiting: bool,
    pub spinner_tick: usize,
    pub slash_matches: Vec<usize>,
    pub slash_selected: usize,
    /// When browsing a command's sub-menu, this holds the index into SLASH_COMMANDS.
    pub active_command: Option<usize>,
    pub sub_matches: Vec<usize>,
    pub sub_selected: usize,
    pub scroll_offset: u16,
    pub banner_text: String,
    pub banner_loading: bool,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            input_history: Vec::new(),
            history_idx: None,
            waiting: false,
            spinner_tick: 0,
            slash_matches: Vec::new(),
            slash_selected: 0,
            active_command: None,
            sub_matches: Vec::new(),
            sub_selected: 0,
            scroll_offset: 0,
            banner_text: random_quote().to_string(),
            banner_loading: false,
        }
    }
}

/// Resolve the state directory for an agent.
pub fn resolve_state_dir(env_name: &str) -> PathBuf {
    crate::config::BASE_DIR.join("agents").join(env_name)
}

/// Default user display name: asterai namespace, falling back to OS username.
pub fn default_user_name() -> String {
    if let Some(ns) = crate::auth::Auth::read_stored_user_namespace()
        && ns != crate::auth::LOCAL_NAMESPACE
    {
        return ns;
    }
    #[cfg(windows)]
    {
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "User".to_string())
    }
    #[cfg(unix)]
    {
        std::env::var("USER").unwrap_or_else(|_| "User".to_string())
    }
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
