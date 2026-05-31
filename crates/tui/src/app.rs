use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io,
    path::PathBuf,
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

use crate::ui;

const REFRESH_RATE: u64 = 100;
const SYS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

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
    // API key status
    pub api_anthropic_ok: bool,
    pub api_gemini_ok: bool,
    pub api_grok_ok: bool,
    pub api_gpt_ok: bool,
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
        let ctx = AppContext::new(
            session_id,
            project_path,
            anthropic_key,
            gemini_key,
            grok_key,
            gpt_key,
        );
        let orchestrator = Orchestrator::new(ctx.clone());
        let (event_tx, event_rx) = mpsc::channel(64);

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
            api_anthropic_ok: has_anthropic,
            api_gemini_ok: has_gemini,
            api_grok_ok: has_grok,
            api_gpt_ok: has_gpt,
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
            if crossterm::event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers).await;
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
            KeyCode::Tab => {
                self.show_logs = !self.show_logs;
            }
            _ => {}
        }
    }

    async fn handle_input(&mut self, input: String) {
        self.log(format!("> {}", input));

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
                self.show_logs = !self.show_logs;
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
                }
            }
            "/help" => {
                self.log("Commands: /clear /reset /status /cost /cancel /retry /log /plan <prompt> /btw <note> /export".to_string());
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
        let tasks = self.orchestrator.plan_from_prompt(&prompt);
        let task_count = tasks.len();
        self.log(format!("Planning {} task(s)…", task_count));

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
            AppEvent::TaskCompleted(task) => {
                self.log(format!("[{}] Done: {} ({} tokens)", &task.id.to_string()[..8], task.description, task.tokens_used));
                if let Some(t) = self.task_history.iter_mut().find(|t| t.id == task.id) {
                    *t = task.clone();
                }
                self.orchestrator.complete_task(task);
            }
            AppEvent::TaskFailed(task) => {
                let err = task.error.clone().unwrap_or_else(|| "unknown error".to_string());
                if err.starts_with("rate_limit:") {
                    self.log(format!(
                        "[rate-limit] {} — quota API dépassé. Tapez /retry pour relancer en local.",
                        task.description
                    ));
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
