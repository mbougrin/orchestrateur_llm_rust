use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use uuid::Uuid;
use sysinfo::System;
use tokenmind_core::{
    context::AppContext,
    orchestrator::Orchestrator,
    task::{Task, TaskStatus},
};
use llm_clients::LlmModel;
use agents;
use file_analyzer;

use crate::ui;

const REFRESH_RATE: u64 = 100;
const SYS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Normal,
    Logs,
    Diff,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelState {
    Idle,
    Downloading,
    Loading,
    Ready,
    Failed(String),
}

impl ModelState {
    pub fn label(&self) -> &str {
        match self {
            ModelState::Idle        => "idle",
            ModelState::Downloading => "downloading…",
            ModelState::Loading     => "loading…",
            ModelState::Ready       => "ready",
            ModelState::Failed(_)   => "failed",
        }
    }
}

pub struct App {
    pub orchestrator: Orchestrator,
    pub ctx: AppContext,
    pub input: String,
    pub cursor_pos: usize,
    pub log_messages: Vec<String>,
    pub show_logs: bool,
    pub task_history: Vec<Task>,
    pub running: bool,
    pub status_message: Option<String>,
    pub model_state: ModelState,
    pub view_mode: ViewMode,
    /// (path, old_content, new_content) — last N file writes
    pub diff_history: Vec<(String, String, String)>,
    // API key status
    pub api_anthropic_ok: bool,
    pub api_gemini_ok: bool,
    pub api_grok_ok: bool,
    pub api_gpt_ok: bool,
    // Streaming: accumulated text per task, cleared on TaskCompleted
    pub active_streams: HashMap<Uuid, String>,
    // Prompt history (T15)
    pub prompt_history: Vec<String>,
    pub history_idx: Option<usize>,
    // /btw model override overlay
    pub btw_overlay: bool,
    pub btw_model_override: Option<LlmModel>,
    // System metrics (refreshed every 2 s)
    pub sys: System,
    pub last_sys_refresh: Instant,
    event_tx: mpsc::Sender<AppEvent>,
    event_rx: mpsc::Receiver<AppEvent>,
}

#[derive(Debug)]
pub enum AppEvent {
    TaskStarted { id: Uuid, description: String, model: String },
    TaskCompleted(Task),
    TaskFailed(Task),
    StreamChunk { task_id: Uuid, chunk: String },
    /// (path, before, after) tuples from BuilderAgent
    FilesWritten(Vec<(String, String, String)>),
    Log(String),
    ModelStatus(ModelState),
}

impl App {
    pub async fn new(
        project_path: PathBuf,
        anthropic_key: String,
        gemini_key: String,
        grok_key: String,
        gpt_key: String,
    ) -> Result<Self> {
        let session_id = Uuid::new_v4().to_string();
        let mut ctx = AppContext::new(
            session_id,
            project_path.clone(),
            anthropic_key,
            gemini_key,
            grok_key,
            gpt_key,
        );

        // Load project memory from ORCHESTRATEUR.md / .orchestrateur/instructions.md / CLAUDE.md
        ctx.project_memory = load_project_memory(&project_path);

        let (event_tx, event_rx) = mpsc::channel(64);

        // Wire up stream sink: agents send (task_id, chunk) here → forwarded as AppEvent::StreamChunk
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel::<(Uuid, String)>();
        ctx.stream_sink = Some(Arc::new(stream_tx));
        {
            let fwd_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some((task_id, chunk)) = stream_rx.recv().await {
                    let _ = fwd_tx.send(AppEvent::StreamChunk { task_id, chunk }).await;
                }
            });
        }

        let orchestrator = Orchestrator::new(ctx.clone());

        let has_anthropic = !ctx.anthropic_key.is_empty() && !ctx.anthropic_key.contains("REPLACE_ME");
        let has_gemini    = !ctx.gemini_key.is_empty()    && !ctx.gemini_key.contains("REPLACE_ME");
        let has_grok      = !ctx.grok_key.is_empty()      && !ctx.grok_key.contains("REPLACE_ME");
        let has_gpt       = !ctx.gpt_key.is_empty()       && !ctx.gpt_key.contains("REPLACE_ME");

        let mut startup_messages = vec!["orchestrateur-llm started. Type your task or /help.".to_string()];

        // Always show key status so the user can confirm what was loaded.
        startup_messages.push(format!(
            "[keys] Anthropic: {}   Gemini: {}   Grok: {}   GPT: {}",
            if has_anthropic { "✓ ok" } else { "✗ absent" },
            if has_gemini    { "✓ ok" } else { "✗ absent" },
            if has_grok      { "✓ ok" } else { "✗ absent" },
            if has_gpt       { "✓ ok" } else { "✗ absent" },
        ));

        if !has_anthropic && has_gemini {
            startup_messages.push(
                "[warn] ANTHROPIC_API_KEY absent — tâches Claude redirigées vers Gemini Flash.".to_string()
            );
        } else if !has_anthropic && !has_gemini {
            startup_messages.push(
                "[warn] Aucune clé API cloud — seul le modèle local (Qwen2.5) sera utilisé.".to_string()
            );
        } else if !has_gemini {
            startup_messages.push(
                "[warn] GEMINI_API_KEY absent — tâches docs redirigées vers Claude Haiku.".to_string()
            );
        }

        // Eager model download/load — runs in background, reports back via the event channel.
        // llama.cpp C-level logs are silenced via llama_log_set before LlamaBackend::init().
        {
            let tx = event_tx.clone();
            tokio::spawn(async move {
                // If the GGUF file is already in the hf-hub cache, the "download" step is
                // instant — show Loading instead of Downloading so the user isn't confused.
                let initial_state = if local_llm::is_model_file_in_cache() {
                    ModelState::Loading
                } else {
                    ModelState::Downloading
                };
                let _ = tx.send(AppEvent::ModelStatus(initial_state.clone())).await;

                // Heartbeat: keep the TUI animated while the blocking work proceeds.
                // Aborted as soon as ensure_ready() returns so it can't fire after
                // model_state has already advanced to Ready/Failed.
                let tx2 = tx.clone();
                let heartbeat_state = initial_state;
                let heartbeat = tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        if tx2.send(AppEvent::ModelStatus(heartbeat_state.clone())).await.is_err() {
                            break;
                        }
                    }
                });

                let result = agents::local_agent::LocalAgent::ensure_ready().await;
                heartbeat.abort();
                match result {
                    Ok(()) => { let _ = tx.send(AppEvent::ModelStatus(ModelState::Ready)).await; }
                    Err(e) => { let _ = tx.send(AppEvent::ModelStatus(ModelState::Failed(e.to_string()))).await; }
                }
            });
        }

        let mut sys = System::new_all();
        sys.refresh_all();

        Ok(Self {
            orchestrator,
            ctx,
            input: String::new(),
            cursor_pos: 0,
            log_messages: startup_messages,
            show_logs: false,
            task_history: Vec::new(),
            running: true,
            status_message: None,
            model_state: ModelState::Idle,
            view_mode: ViewMode::Normal,
            diff_history: Vec::new(),
            api_anthropic_ok: has_anthropic,
            api_gemini_ok: has_gemini,
            api_grok_ok: has_grok,
            api_gpt_ok: has_gpt,
            active_streams: HashMap::new(),
            prompt_history: Vec::new(),
            history_idx: None,
            btw_overlay: false,
            btw_model_override: None,
            sys,
            last_sys_refresh: Instant::now(),
            event_tx,
            event_rx,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let tick_rate = Duration::from_millis(REFRESH_RATE);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|f| ui::draw(f, self))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());

            // Poll for keyboard events OR Ctrl+C signal
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => {
                    self.log("[session] Ctrl+C — session pausée. Tapez /resume pour reprendre ou ESC pour quitter.".to_string());
                    self.status_message = Some("Session pausée (Ctrl+C) — /resume ou ESC".to_string());
                    // Don't exit — let the user choose
                }
                _ = tokio::time::sleep(Duration::ZERO) => {
                    if crossterm::event::poll(timeout)? {
                        if let Event::Key(key) = event::read()? {
                            self.handle_key(key.code, key.modifiers).await;
                        }
                    }
                }
            }

            // Drain async events
            while let Ok(evt) = self.event_rx.try_recv() {
                self.handle_app_event(evt);
            }

            // Dispatch any tasks that became ready this tick
            self.dispatch_pending().await;

            // Refresh CPU/RAM/Swap every 2 s
            if self.last_sys_refresh.elapsed() >= SYS_REFRESH_INTERVAL {
                self.sys.refresh_cpu_usage();
                self.sys.refresh_memory();
                self.last_sys_refresh = Instant::now();
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }

            if !self.running {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
        terminal.show_cursor()?;
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        // /btw overlay consumes keys before the rest of the UI
        if self.btw_overlay {
            match key {
                KeyCode::Char('1') => {
                    self.btw_model_override = Some(LlmModel::ClaudeSonnet);
                    self.btw_overlay = false;
                    self.log("[btw] Override : claude-sonnet-4-5".to_string());
                }
                KeyCode::Char('2') => {
                    self.btw_model_override = Some(LlmModel::ClaudeHaiku);
                    self.btw_overlay = false;
                    self.log("[btw] Override : claude-haiku-4-5".to_string());
                }
                KeyCode::Char('3') => {
                    self.btw_model_override = Some(LlmModel::Gemini);
                    self.btw_overlay = false;
                    self.log("[btw] Override : gemini-2.0-flash".to_string());
                }
                KeyCode::Char('0') => {
                    self.btw_model_override = None;
                    self.btw_overlay = false;
                    self.log("[btw] Override supprimé — routage automatique.".to_string());
                }
                KeyCode::Esc => { self.btw_overlay = false; }
                _ => {}
            }
            return;
        }

        match key {
            KeyCode::Esc => self.running = false,
            KeyCode::Enter => {
                let input = self.input.trim().to_string();
                if !input.is_empty() {
                    self.input.clear();
                    self.cursor_pos = 0;
                    self.handle_input(input).await;
                }
            }
            KeyCode::Char(c) => {
                if modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                    self.running = false;
                } else {
                    self.input.insert(self.cursor_pos, c);
                    self.cursor_pos += 1;
                }
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.input.remove(self.cursor_pos);
                }
            }
            KeyCode::Left => {
                if self.cursor_pos > 0 { self.cursor_pos -= 1; }
            }
            KeyCode::Right => {
                if self.cursor_pos < self.input.len() { self.cursor_pos += 1; }
            }
            // T15: prompt history navigation
            KeyCode::Up => {
                if self.prompt_history.is_empty() { return; }
                let next_idx = match self.history_idx {
                    None => self.prompt_history.len() - 1,
                    Some(0) => 0,
                    Some(i) => i - 1,
                };
                self.history_idx = Some(next_idx);
                self.input = self.prompt_history[next_idx].clone();
                self.cursor_pos = self.input.len();
            }
            KeyCode::Down => {
                match self.history_idx {
                    None => {}
                    Some(i) if i + 1 >= self.prompt_history.len() => {
                        self.history_idx = None;
                        self.input.clear();
                        self.cursor_pos = 0;
                    }
                    Some(i) => {
                        let next = i + 1;
                        self.history_idx = Some(next);
                        self.input = self.prompt_history[next].clone();
                        self.cursor_pos = self.input.len();
                    }
                }
            }
            KeyCode::Tab => {
                self.view_mode = match self.view_mode {
                    ViewMode::Normal => ViewMode::Logs,
                    ViewMode::Logs   => ViewMode::Diff,
                    ViewMode::Diff   => ViewMode::Normal,
                };
                self.show_logs = self.view_mode == ViewMode::Logs;
            }
            _ => {}
        }
    }

    async fn handle_input(&mut self, input: String) {
        self.log(format!("> {}", input));
        self.history_idx = None;

        // T15: save to prompt history (cap at 200 entries)
        if !input.starts_with('/') || input.len() > 1 {
            if self.prompt_history.last().map(|s| s.as_str()) != Some(&input) {
                self.prompt_history.push(input.clone());
                if self.prompt_history.len() > 200 {
                    self.prompt_history.remove(0);
                }
            }
        }

        if input.starts_with('/') {
            self.handle_slash_command(&input).await;
        } else {
            self.handle_task_prompt(input).await;
        }
    }

    async fn handle_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
        match parts[0] {
            "/clear" => {
                // T18: auto-summary before clearing
                let done = self.task_history.iter().filter(|t| t.status == TaskStatus::Done).count();
                let failed = self.task_history.iter().filter(|t| t.status == TaskStatus::Failed).count();
                if done + failed > 0 {
                    let names: Vec<String> = self.task_history.iter()
                        .filter(|t| t.status == TaskStatus::Done)
                        .take(5)
                        .map(|t| t.description.chars().take(30).collect::<String>())
                        .collect();
                    let cost = self.ctx.total_cost();
                    self.log(format!(
                        "[session] {} done / {} failed — cost ${:.4} — dernières: {}",
                        done, failed, cost, names.join("; ")
                    ));
                }
                self.task_history.clear();
                self.diff_history.clear();
                self.active_streams.clear();
                self.log("Context cleared.".to_string());
                self.status_message = Some("Context cleared".to_string());
            }
            "/reset" => {
                self.task_history.clear();
                self.log("Session reset.".to_string());
            }
            "/status" => {
                let pending = self.task_history.iter().filter(|t| t.status == TaskStatus::Pending).count();
                let done = self.task_history.iter().filter(|t| t.status == TaskStatus::Done).count();
                let failed = self.task_history.iter().filter(|t| t.status == TaskStatus::Failed).count();
                self.log(format!("Tasks: {} pending / {} done / {} failed", pending, done, failed));
            }
            "/cost" => {
                let total = self.ctx.total_cost();
                let savings = self.ctx.savings_percent();
                self.log(format!(
                    "Total cost: ${:.4} | Saved vs all-Sonnet: {:.0}%",
                    total, savings
                ));
                for model in [LlmModel::ClaudeSonnet, LlmModel::ClaudeHaiku, LlmModel::Gemini, LlmModel::Grok, LlmModel::Gpt, LlmModel::Local] {
                    let tokens = self.ctx.token_count(&model);
                    if tokens > 0 {
                        self.log(format!("  {}: {} tokens", model.display_name(), tokens));
                    }
                }
            }
            "/cancel" => {
                self.orchestrator.cancel_pending();
                self.log("Pending tasks cancelled.".to_string());
            }
            "/retry" => {
                let fallbacks: Vec<Task> = self.task_history.iter()
                    .filter(|t| {
                        t.status == TaskStatus::Failed &&
                        t.error.as_deref().unwrap_or("").starts_with("rate_limit:")
                    })
                    .cloned()
                    .collect();

                if fallbacks.is_empty() {
                    self.log("[retry] Aucune tâche rate-limitée à relancer.".to_string());
                } else {
                    let count = fallbacks.len();
                    let mut to_enqueue = Vec::new();
                    for mut task in fallbacks {
                        task.assigned_model = LlmModel::Local;
                        task.status = TaskStatus::Pending;
                        task.error = None;
                        task.retry_count = 0;
                        if let Some(t) = self.task_history.iter_mut().find(|t| t.id == task.id) {
                            *t = task.clone();
                        }
                        to_enqueue.push(task);
                    }
                    self.orchestrator.enqueue_tasks(to_enqueue);
                    self.log(format!("[retry] {} tâche(s) relancée(s) en local.", count));
                }
            }
            "/log" => {
                self.view_mode = ViewMode::Logs;
                self.show_logs = true;
            }
            "/diff" => {
                self.view_mode = ViewMode::Diff;
                self.show_logs = false;
                if self.diff_history.is_empty() {
                    self.log("[diff] Aucun diff disponible pour le moment.".to_string());
                }
            }
            "/plan" => {
                if parts.len() > 1 {
                    let prompt = parts[1..].join(" ");
                    let tasks = self.orchestrator.plan_from_prompt(&prompt);
                    self.log(format!("Plan ({} tasks):", tasks.len()));
                    for task in &tasks {
                        self.log(format!("  [{:?}] {} → {}", task.task_type, task.description, task.assigned_model.display_name()));
                    }
                }
            }
            "/btw" => {
                if parts.len() > 1 {
                    self.log(format!("[note] {}", parts[1..].join(" ")));
                } else {
                    self.btw_overlay = !self.btw_overlay;
                    if self.btw_overlay {
                        self.log("[btw] 1=Sonnet  2=Haiku  3=Gemini  0=auto  ESC=annuler".to_string());
                    }
                }
            }
            "/autowrite" => {
                match parts.get(1).copied().unwrap_or("") {
                    "on"  => {
                        self.ctx.auto_write = true;
                        self.log("[autowrite] Écriture automatique activée.".to_string());
                    }
                    "off" => {
                        self.ctx.auto_write = false;
                        self.log("[autowrite] Écriture désactivée — /diff pour réviser avant écriture.".to_string());
                    }
                    _ => { self.log("[autowrite] Usage : /autowrite on|off".to_string()); }
                }
            }
            "/add" => {
                let path_arg = parts.get(1).copied().unwrap_or("").to_string();
                if path_arg.is_empty() {
                    self.log("[add] Usage : /add <chemin>".to_string());
                } else {
                    let full = if std::path::Path::new(&path_arg).is_absolute() {
                        PathBuf::from(&path_arg)
                    } else {
                        self.ctx.project_path.join(&path_arg)
                    };
                    let msg = match std::fs::read_to_string(&full) {
                        Ok(content) => {
                            let msg = if let Ok(mut manual) = self.ctx.manual_context.lock() {
                                if let Some(entry) = manual.iter_mut().find(|(p, _)| p == &path_arg) {
                                    entry.1 = content.clone();
                                    format!("[add] Mis à jour : {} ({} chars)", path_arg, content.len())
                                } else {
                                    manual.push((path_arg.clone(), content.clone()));
                                    format!("[add] Ajouté : {} ({} chars)", path_arg, content.len())
                                }
                            } else {
                                "[add] Erreur verrou manual_context.".to_string()
                            };
                            msg
                        }
                        Err(e) => format!("[add] Erreur lecture {} : {}", path_arg, e),
                    };
                    self.log(msg);
                }
            }
            "/context" => {
                let sub = parts.get(1).copied().unwrap_or("");
                match sub {
                    "list" => {
                        let lines: Vec<String> = if let Ok(manual) = self.ctx.manual_context.lock() {
                            if manual.is_empty() {
                                vec!["[context] Aucun fichier en contexte manuel.".to_string()]
                            } else {
                                manual.iter()
                                    .map(|(p, c)| format!("  {} ({} chars)", p, c.len()))
                                    .collect()
                            }
                        } else {
                            vec!["[context] Erreur verrou.".to_string()]
                        };
                        for line in lines { self.log(line); }
                    }
                    "rm" => {
                        let path_arg = parts.get(2).copied().unwrap_or("").to_string();
                        if path_arg.is_empty() {
                            self.log("[context] Usage : /context rm <chemin>".to_string());
                        } else {
                            let msg = if let Ok(mut manual) = self.ctx.manual_context.lock() {
                                let before = manual.len();
                                manual.retain(|(p, _)| p != &path_arg);
                                if manual.len() < before {
                                    format!("[context] Retiré : {}", path_arg)
                                } else {
                                    format!("[context] Non trouvé : {}", path_arg)
                                }
                            } else {
                                "[context] Erreur verrou.".to_string()
                            };
                            self.log(msg);
                        }
                    }
                    _ => { self.log("[context] Sous-commandes : list | rm <chemin>".to_string()); }
                }
            }
            "/memory" => {
                let sub = parts.get(1).copied().unwrap_or("");
                match sub {
                    "show" => {
                        if self.ctx.project_memory.is_empty() {
                            self.log("[memory] Aucune mémoire projet (ORCHESTRATEUR.md absent).".to_string());
                        } else {
                            self.log(format!("[memory]\n{}", self.ctx.project_memory));
                        }
                    }
                    "clear" => {
                        let path = self.ctx.project_path.join("ORCHESTRATEUR.md");
                        let _ = std::fs::write(&path, "");
                        self.ctx.project_memory.clear();
                        self.log("[memory] ORCHESTRATEUR.md vidé.".to_string());
                    }
                    "add" => {
                        let note = parts.get(2..).map(|p| p.join(" ")).unwrap_or_default();
                        if note.is_empty() {
                            self.log("[memory] Usage : /memory add <texte>".to_string());
                        } else {
                            let path = self.ctx.project_path.join("ORCHESTRATEUR.md");
                            let mut content = std::fs::read_to_string(&path).unwrap_or_default();
                            if !content.ends_with('\n') && !content.is_empty() { content.push('\n'); }
                            content.push_str(&format!("- {}\n", note));
                            let _ = std::fs::write(&path, &content);
                            self.ctx.project_memory = content;
                            self.log(format!("[memory] Ajouté : {}", note));
                        }
                    }
                    _ => {
                        self.log("[memory] Sous-commandes : add <texte> | show | clear".to_string());
                    }
                }
            }
            "/git" => {
                let sub = parts.get(1).copied().unwrap_or("");
                match sub {
                    "status" => {
                        let git = file_analyzer::git_context(&self.ctx.project_path);
                        self.ctx.git = git.clone();
                        if git.is_empty() {
                            self.log("[git] Pas de dépôt git dans ce répertoire.".to_string());
                        } else {
                            self.log(format!("[git] Branche: {}", git.branch));
                            for (path, status) in &git.status {
                                self.log(format!("  {} {}", status, path));
                            }
                        }
                    }
                    "commit" => {
                        // Refresh git context, then queue a local task to generate commit msg
                        let git = file_analyzer::git_context(&self.ctx.project_path);
                        self.ctx.git = git.clone();
                        if git.status.is_empty() {
                            self.log("[git] Rien à committer (working tree clean).".to_string());
                        } else {
                            let diff_summary = git.status.iter()
                                .map(|(p, c)| format!("{} {}", c, p))
                                .collect::<Vec<_>>().join(", ");
                            self.log(format!("[git] Génération du message de commit pour : {}", diff_summary));
                            let prompt = format!(
                                "Generate a concise git commit message (50 chars max subject, \
                                imperative mood) for these changes:\n{}\n\nDiff:\n{}",
                                diff_summary, &git.recent_diff
                            );
                            // Enqueue as a Low-priority local task
                            let mut tasks = self.orchestrator.plan_from_prompt(&prompt);
                            for t in &mut tasks {
                                t.assigned_model = LlmModel::Local;
                            }
                            let count = tasks.len();
                            self.orchestrator.enqueue_tasks(tasks.clone());
                            for t in tasks { self.task_history.push(t); }
                            self.log(format!("[git] {} tâche(s) de commit queued.", count));
                        }
                    }
                    "log" => {
                        let git = file_analyzer::git_context(&self.ctx.project_path);
                        for c in &git.last_commits {
                            self.log(format!("  {}", c));
                        }
                    }
                    _ => {
                        self.log("[git] Sous-commandes : status | commit | log".to_string());
                    }
                }
            }
            "/verbose" => {
                self.ctx.verbose = !self.ctx.verbose;
                if self.ctx.verbose {
                    self.log("[verbose] Mode verbeux activé — réponses LLM complètes dans les logs.".to_string());
                } else {
                    self.log("[verbose] Mode verbeux désactivé.".to_string());
                }
            }
            "/profile" => {
                use tokenmind_core::context::CostProfile;
                let sub = parts.get(1).copied().unwrap_or("");
                match sub {
                    "quality" => {
                        self.ctx.profile = CostProfile::Quality;
                        self.orchestrator.ctx.profile = CostProfile::Quality;
                        self.log("[profile] Quality — Sonnet pour tout, Haiku pour les tâches Low.".to_string());
                    }
                    "balanced" => {
                        self.ctx.profile = CostProfile::Balanced;
                        self.orchestrator.ctx.profile = CostProfile::Balanced;
                        self.log("[profile] Balanced — routage automatique par priorité (défaut).".to_string());
                    }
                    "cheap" => {
                        self.ctx.profile = CostProfile::Cheap;
                        self.orchestrator.ctx.profile = CostProfile::Cheap;
                        self.log("[profile] Cheap — toutes les tâches en local (coût $0).".to_string());
                    }
                    _ => {
                        let cur = match self.ctx.profile {
                            CostProfile::Quality  => "quality",
                            CostProfile::Balanced => "balanced",
                            CostProfile::Cheap    => "cheap",
                        };
                        self.log(format!("[profile] Actuel : {}  — options : quality | balanced | cheap", cur));
                    }
                }
            }
            "/doctor" => {
                self.log("[doctor] Diagnostic système :".to_string());
                // API keys
                self.log(format!("  Anthropic : {}", if self.api_anthropic_ok { "✓ ok" } else { "✗ absent" }));
                self.log(format!("  Gemini    : {}", if self.api_gemini_ok    { "✓ ok" } else { "✗ absent" }));
                self.log(format!("  Grok      : {}", if self.api_grok_ok      { "✓ ok" } else { "✗ absent" }));
                self.log(format!("  GPT       : {}", if self.api_gpt_ok       { "✓ ok" } else { "✗ absent" }));
                // Local model
                let local_status = match &self.model_state {
                    ModelState::Ready        => "✓ prêt",
                    ModelState::Loading      => "… chargement",
                    ModelState::Downloading  => "… téléchargement",
                    ModelState::Failed(_)    => "✗ échec",
                    ModelState::Idle         => "○ idle",
                };
                self.log(format!("  Local LLM : {}", local_status));
                // Project path
                let path = self.ctx.project_path.clone();
                let path_ok = path.exists() && path.is_dir();
                let writable = std::fs::write(path.join(".orch_probe"), b"").map(|_| {
                    let _ = std::fs::remove_file(path.join(".orch_probe"));
                    true
                }).unwrap_or(false);
                self.log(format!("  Projet    : {} {}", path.display(),
                    if path_ok && writable { "✓" } else if path_ok { "⚠ lecture seule" } else { "✗ introuvable" }));
                // cargo in PATH
                let cargo_ok = std::process::Command::new("cargo").arg("--version")
                    .output().map(|o| o.status.success()).unwrap_or(false);
                self.log(format!("  cargo     : {}", if cargo_ok { "✓ ok" } else { "✗ non trouvé" }));
                // git in PATH
                let git_ok = std::process::Command::new("git").arg("--version")
                    .output().map(|o| o.status.success()).unwrap_or(false);
                self.log(format!("  git       : {}", if git_ok { "✓ ok" } else { "✗ non trouvé" }));
                // Context manual
                let manual_count = self.ctx.manual_context.lock().map(|m| m.len()).unwrap_or(0);
                self.log(format!("  /add      : {} fichier(s) en contexte manuel", manual_count));
                // Profile + verbose
                use tokenmind_core::context::CostProfile;
                let profile_str = match self.ctx.profile {
                    CostProfile::Quality  => "quality",
                    CostProfile::Balanced => "balanced",
                    CostProfile::Cheap    => "cheap",
                };
                self.log(format!("  Profil    : {}  verbose: {}", profile_str, if self.ctx.verbose { "on" } else { "off" }));
            }
            "/resume" => {
                self.log("[resume] Vérification des tâches en attente…".to_string());
                let pending = self.task_history.iter().filter(|t| t.status == TaskStatus::Pending).count();
                if pending > 0 {
                    self.log(format!("[resume] {} tâche(s) en attente, relancement…", pending));
                } else {
                    self.log("[resume] Aucune tâche en attente.".to_string());
                }
            }
            "/help" => {
                self.log("Commands: /clear /reset /status /cost /cancel /retry /log /diff /plan <prompt>".to_string());
                self.log("          /btw [note]  /autowrite on|off  /add <path>  /context list|rm <path>".to_string());
                self.log("          /verbose  /profile quality|balanced|cheap  /doctor".to_string());
                self.log("          /memory add|show|clear  /git status|commit|log  /resume  /export".to_string());
                self.log("          ↑/↓ navigate prompt history".to_string());
            }
            "/local" => {
                if parts.len() > 1 {
                    self.log(format!("Local LLM: {}", parts[1]));
                }
            }
            "/export" => {
                let report = self.generate_session_report();
                self.log(format!("Session report:\n{}", report));
            }
            _ => {
                self.log(format!("Unknown command: {}. Type /help.", parts[0]));
            }
        }
    }

    async fn handle_task_prompt(&mut self, prompt: String) {
        let mut tasks = self.orchestrator.plan_from_prompt(&prompt);
        let task_count = tasks.len();
        self.log(format!("Planning {} task(s)…", task_count));

        // Apply /btw model override if active (single-use, then clear)
        if let Some(ref model) = self.btw_model_override.clone() {
            for task in &mut tasks {
                task.assigned_model = model.clone();
            }
            self.log(format!("[btw] Modèle forcé pour {} tâche(s) : {}", task_count, model.display_name()));
            self.btw_model_override = None;
        }

        self.orchestrator.enqueue_tasks(tasks.clone());

        for task in tasks {
            self.task_history.push(task);
        }

        self.log(format!("{} task(s) queued. Processing…", task_count));
        self.status_message = Some(format!("{} task(s) queued", task_count));
    }

    fn handle_app_event(&mut self, evt: AppEvent) {
        match evt {
            AppEvent::Log(msg) => self.log(msg),
            AppEvent::TaskStarted { id, description, model } => {
                self.log(format!("[{}] Starting: {} → {}", &id.to_string()[..8], description, model));
                if let Some(t) = self.task_history.iter_mut().find(|t| t.id == id) {
                    t.mark_running();
                }
            }
            AppEvent::FilesWritten(diffs) => {
                let count = diffs.len();
                self.diff_history.extend(diffs);
                // Keep last 20 file diffs
                if self.diff_history.len() > 20 {
                    self.diff_history.drain(0..self.diff_history.len() - 20);
                }
                self.log(format!("[diff] {} fichier(s) modifié(s). Tab→Diff pour voir.", count));
            }
            AppEvent::StreamChunk { task_id, chunk } => {
                self.active_streams.entry(task_id).or_default().push_str(&chunk);
            }
            AppEvent::TaskCompleted(task) => {
                self.active_streams.remove(&task.id);
                self.log(format!("[{}] Done: {} ({} tokens)", &task.id.to_string()[..8], task.description, task.tokens_used));
                if let Some(t) = self.task_history.iter_mut().find(|t| t.id == task.id) {
                    *t = task.clone();
                }
                self.orchestrator.complete_task(task);
            }
            AppEvent::TaskFailed(task) => {
                self.active_streams.remove(&task.id);
                let err = task.error.clone().unwrap_or_else(|| "unknown error".to_string());
                if err.starts_with("rate_limit:") {
                    self.log(format!(
                        "[rate-limit] {} — quota API dépassé. Tapez /retry pour relancer en local.",
                        task.description
                    ));
                } else if err.starts_with("test_error:") {
                    self.log(format!("[tests] {} — tests échoués, erreurs renvoyées au coder.", task.description));
                } else if err.starts_with("build_error:") {
                    self.log(format!("[build] {} — erreur de compilation, relance prévue.", task.description));
                } else {
                    self.log(format!("[{}] Failed: {} — {}", &task.id.to_string()[..8], task.description, err));
                }
                if let Some(t) = self.task_history.iter_mut().find(|t| t.id == task.id) {
                    *t = task.clone();
                }
                self.orchestrator.complete_task(task);
            }
            AppEvent::ModelStatus(state) => {
                if state == self.model_state { return; } // suppress duplicate heartbeats
                let msg = match &state {
                    ModelState::Downloading => "[local] Téléchargement Qwen2.5-Coder…".to_string(),
                    ModelState::Loading     => "[local] Chargement Qwen2.5-Coder en mémoire…".to_string(),
                    ModelState::Ready       => "[local] Modèle Qwen2.5-Coder prêt.".to_string(),
                    ModelState::Failed(e)   => format!("[local] Échec chargement modèle : {}", e),
                    ModelState::Idle        => return,
                };
                self.log(msg);
                self.model_state = state;
            }
        }
    }

    pub fn log(&mut self, msg: String) {
        self.log_messages.push(msg);
        if self.log_messages.len() > 500 {
            self.log_messages.remove(0);
        }
    }

    /// Dequeue every task that is ready to run and spawn a background worker for each.
    async fn dispatch_pending(&mut self) {
        while let Some(task) = self.orchestrator.next_ready_task() {
            let id    = task.id;
            let model = task.assigned_model.clone();
            let desc  = task.description.clone();
            let tx    = self.event_tx.clone();
            let ctx   = self.ctx.clone();

            // Reflect Running state in the UI immediately (before the event round-trip).
            if let Some(t) = self.task_history.iter_mut().find(|t| t.id == id) {
                t.mark_running();
            }

            tokio::spawn(async move {
                let _ = tx.send(AppEvent::TaskStarted {
                    id,
                    description: desc,
                    model: model.display_name().to_string(),
                }).await;

                let mut task = task;

                let agent: Box<dyn agents::Agent> = match &model {
                    LlmModel::ClaudeSonnet => Box::new(agents::rust_agent::RustAgent),
                    LlmModel::ClaudeHaiku  => Box::new(agents::frontend_agent::FrontendAgent),
                    LlmModel::Gemini       => Box::new(agents::doc_agent::DocAgent),
                    LlmModel::Grok         => Box::new(agents::grok_agent::GrokAgent),
                    LlmModel::Gpt          => Box::new(agents::gpt_agent::GptAgent),
                    LlmModel::Local        => Box::new(agents::local_agent::LocalAgent::new(0.6)),
                };

                match agent.execute(&mut task, &ctx).await {
                    Ok(_) => {
                        // T19: propagate file diffs to the TUI diff panel
                        if !task.file_diffs.is_empty() {
                            let _ = tx.send(AppEvent::FilesWritten(task.file_diffs.clone())).await;
                        }
                        let _ = tx.send(AppEvent::TaskCompleted(task)).await;
                    }
                    Err(e) => {
                        task.mark_failed(e.to_string());
                        let _ = tx.send(AppEvent::TaskFailed(task)).await;
                    }
                }
            });
        }
    }

    pub fn has_active_stream(&self) -> bool {
        !self.active_streams.is_empty()
    }

    fn generate_session_report(&self) -> String {
        let mut report = format!("# orchestrateur-llm Session Report\n\nProject: {}\n\n", self.ctx.project_path.display());
        report.push_str(&format!("Total cost: ${:.4}\n", self.ctx.total_cost()));
        report.push_str(&format!("Savings vs all-Sonnet: {:.0}%\n\n", self.ctx.savings_percent()));
        report.push_str("## Tasks\n\n");
        for task in &self.task_history {
            report.push_str(&format!(
                "- [{:?}] {} → {:?} (tokens: {})\n",
                task.status, task.description, task.assigned_model, task.tokens_used
            ));
        }
        report
    }
}

/// Look for ORCHESTRATEUR.md / .orchestrateur/instructions.md / CLAUDE.md in project root.
fn load_project_memory(project_path: &std::path::Path) -> String {
    let candidates = [
        project_path.join("ORCHESTRATEUR.md"),
        project_path.join(".orchestrateur").join("instructions.md"),
        project_path.join("CLAUDE.md"),
    ];
    for path in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            if !content.trim().is_empty() {
                tracing::info!("[memory] Loaded project memory from {}", path.display());
                return content;
            }
        }
    }
    String::new()
}
