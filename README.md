# orchestrateur-llm

> Orchestrateur CLI/TUI qui divise automatiquement les tâches de développement entre plusieurs LLM pour minimiser les coûts en tokens tout en maximisant la qualité du code produit.

**Principe** : chaque tâche est classifiée par priorité (High / Medium / Low) et routée vers le LLM le plus adapté. Le modèle local tourne **en mémoire sans serveur** via `llama-cpp-4`. Les réponses LLM s'affichent en temps réel dans le TUI via SSE streaming.

---

## Modèles et coûts

| Priorité | Modèle | Cas d'usage | Coût entrée / 1M tok |
|---|---|---|---|
| **High** | `claude-sonnet-4-5` | Rust, Tauri, planification, build fix | $3.00 |
| **Medium** | `claude-haiku-4-5` | Vue.js, HTML, TypeScript, CSS, frontend | $0.25 |
| **Medium** | `grok-3-mini` | Fallback Haiku — API OpenAI-compatible | $0.30 |
| **Medium/High** | `gemini-2.0-flash` | Fallback Sonnet / docs, scripts | ~gratuit (free tier) |
| **Medium/High** | `gpt-4o-mini` | Fallback GPT — OpenAI | $0.15 |
| **Low** | `qwen2.5-coder` (local) | .md, .json, .sh, boilerplate, typos | **$0.00** |

### Chaîne de fallback par priorité

```
High   : Sonnet  → Gemini  → Grok
Medium : Haiku   → Grok    → Gemini → Local
Low    : Local   → Grok
```

Configurable dans `~/.config/orchestrateur-llm/config.toml`.

---

## Interface TUI

```
 orchestrateur-llm v1.0  [my-app]  [cost: $0.0142]  [local: ready]  [Tab: cycle]  [ESC: quit]
┌────────────────────────────────────────────┬─────────────────────────────────────────────────┐
│  STATUS SYSTÈME                            │  QUEUE DES TÂCHES                               │
│                                            │                                                 │
│  Anthropic  ✓ ok                           │  [✓] Analyze project structure (sonnet) [842tok]│
│  Gemini     ✓ ok                           │  [✓] Plan tasks (local)          [~120tok]       │
│  Grok       ✗ absent                       │  [►] impl Display for Config     (sonnet)        │
│  GPT        ✗ absent                       │  [►] Update README               (gemini)        │
│  Local      ✓ ready                        │  [○] Build validation            (local)         │
│                                            │                                                 │
│  CPU  [████░░░░]  42%                      │  ┌──── STREAM ──────────────────────────────┐   │
│  RAM  [███░░░░░]  8.2 GB                   │  │ impl Display for Config {                │   │
│  Swap [░░░░░░░░]  0 MB                     │  │   fn fmt(&self, f: &mut Formatter) -> .. │   │
│                                            │  │ ▌                                        │   │
│  TOKEN USAGE                               │  └──────────────────────────────────────────┘   │
│  Sonnet  ████░  1 284                      │                                                 │
│  Haiku   ██░░░    456                      │                                                 │
│  Gemini  █░░░░     89                      │                                                 │
│  Local   ──── FREE                         │                                                 │
│  Total: ~$0.0142  Saved: ~73%              │                                                 │
└────────────────────────────────────────────┴─────────────────────────────────────────────────┘
│  > █                                                                                         │
│  /btw /clear /status /cost /plan /verbose /profile /doctor /add /help                       │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Navigation clavier

| Touche | Action |
|---|---|
| `Tab` | Cycle entre les vues : **Normal → Logs → Diff → Normal** |
| `↑` / `↓` | Naviguer dans l'historique des prompts |
| `ESC` | Quitter |
| `Ctrl+C` | Mettre en pause la session (pas de fermeture — tapez `/resume` ou `ESC`) |

### Vues disponibles (Tab)

- **Normal** — queue des tâches + panneau de streaming en direct
- **Logs** — journal complet des événements système
- **Diff** — diff unifié coloré (vert `+`, rouge `-`) des derniers fichiers modifiés

---

## Architecture des crates

```
crates/
├── core/
│   ├── task.rs              Task · TaskType · TaskStatus · Priority (High/Medium/Low)
│   ├── orchestrator.rs      Orchestrateur — planification + profil de coût
│   ├── dispatcher.rs        Routing par priorité + disponibilité des clés API
│   ├── context.rs           AppContext (clés, stream_sink, git, manual_context, profile…)
│   └── llm_arch_config.rs   LlmArchConfig — lu depuis config.toml
│
├── agents/
│   ├── rust_agent.rs        RustAgent      → Sonnet (code Rust / Tauri)
│   ├── frontend_agent.rs    FrontendAgent  → Haiku  (Vue, TS, CSS)
│   ├── doc_agent.rs         DocAgent       → Gemini  (Markdown, scripts)
│   ├── grok_agent.rs        GrokAgent      → Grok    (xAI, OpenAI-compatible)
│   ├── gpt_agent.rs         GptAgent       → GPT-4o-mini (OpenAI)
│   ├── local_agent.rs       LocalAgent     → llama-cpp-4 in-process (Qwen2.5-Coder)
│   ├── builder_agent.rs     BuilderAgent   — parse le patch-format LLM et écrit les fichiers
│   ├── build_agent.rs       BuildValidator — cargo check / npm type-check / go build / pytest
│   ├── verify_agent.rs      VerifyAgent    — lance BuildValidator, renvoie erreurs au coder
│   ├── context_builder.rs   build_rich_context() — arbre projet + git + imports 2 niveaux
│   └── shell_tool.rs        ShellTool — exécute les directives TOOL: (grep/find/git_diff…)
│
├── llm_clients/
│   ├── anthropic.rs         Claude SSE streaming + détection 429 → "rate_limit:"
│   ├── gemini.rs            Gemini SSE streaming + détection 429
│   ├── grok.rs              Grok-3-mini  xAI
│   ├── gpt.rs               GPT-4o-mini  OpenAI
│   └── lib.rs               StreamEvent · StreamSink · estimate_tokens() · TokenUsage
│
├── file_analyzer/
│   ├── imports.rs           parse_imports() · resolve_deps() · build_project_tree()
│   └── git_context.rs       GitContext — branche, status, diff récent (git2)
│
├── context_store/           Persistance SQLite (checkpoints, prompt_history, manual_context)
├── local_llm/               Inférence llama-cpp-4 in-process (téléchargement HF automatique)
└── tui/
    ├── app.rs               App state · event loop · dispatch · slash commands
    ├── ui.rs                draw() — layout header/body/input + overlay /btw
    └── components/
        ├── status_panel.rs  Clés API + métriques système (CPU/RAM)
        ├── token_counter.rs Usage tokens par modèle + coût total
        ├── log_panel.rs     Journal plein écran (vue Logs)
        ├── diff_panel.rs    Diff unifié coloré (vue Diff) — crate `similar`
        └── stream_panel.rs  Texte en cours de génération + curseur clignotant ▌
```

---

## Pipeline d'exécution

```
Prompt utilisateur
        │
        ▼
Orchestrator.plan_from_prompt()
  • classification (RustCode / FrontendCode / DocUpdate / …)
  • routing par priorité + disponibilité des clés
  • application du profil de coût (/profile)
  • override /btw si actif
        │
        ▼
Agent codeur (Sonnet / Haiku / Gemini / Grok / GPT / Local)
  • contexte riche : arbre projet + git + fichiers cibles + imports 2 niveaux
  • mémoire projet (ORCHESTRATEUR.md) en préfixe système
  • tool loop : directives TOOL:grep/find/git_diff exécutées et réinjectées (max 3)
  • streaming SSE → StreamPanel en direct
  • estimation tokens avant appel → affichée dans la queue
        │
        ▼ write_and_track()
BuilderAgent (intégré dans chaque agent codeur)
  • parse le patch-format <<<FILE:path:CREATE>>> … <<<END>>>
  • écrit tous les fichiers en un seul passage
  • stocke les diffs (old, new) dans task.file_diffs
  • vérifie le ratio de changement si auto_write = off
        │
        ▼
VerifyAgent → BuildValidator
  • cargo check / cargo test / npm type-check / pytest / go test
  • si erreur → renvoyée au coder (retry, max 3)
        │
        ▼
FilesWritten event → diff_history → vue Diff (Tab)
```

---

## Format de patch LLM

Tous les agents codeurs envoient ce format dans leur system prompt :

```
Créer un nouveau fichier :
<<<FILE:path/to/file.rs:CREATE>>>
<contenu complet>
<<<END>>>

Remplacer les lignes 42–67 d'un fichier existant :
<<<FILE:src/main.rs:REPLACE:42:67>>>
<code de remplacement>
<<<END>>>

Plusieurs fichiers dans une même réponse → un bloc par fichier.
```

Fallback : les blocs ` ```lang:path/to/file` sont aussi reconnus (rétro-compat).

---

## Contexte riche — ce que voit le LLM

À chaque appel, les agents construisent un contexte de 32 000 chars max comprenant :

1. **Arbre projet** (3 niveaux, sans `target/` / `node_modules/`)
2. **Contexte git** — branche, fichiers modifiés, diff récent, 5 derniers commits
3. **Fichiers manuels** — tout ce qui a été ajouté via `/add <path>`
4. **Fichiers cibles** — contenu complet des fichiers mentionnés dans la tâche
5. **Imports niveau 1** — dépendances directes, contenu complet
6. **Imports niveau 2** — dépendances des dépendances, **signatures publiques seulement** (`pub fn`, `pub struct`, `pub enum`, `pub trait`, `pub type`, `pub const`)

---

## Mémoire projet — ORCHESTRATEUR.md

Au démarrage, l'orchestrateur cherche (dans l'ordre) :

```
ORCHESTRATEUR.md
.orchestrateur/instructions.md
CLAUDE.md
```

Le contenu est injecté dans le **préfixe système** de chaque appel LLM. Gérez-le avec :

```
/memory add <note>   — ajoute une ligne à ORCHESTRATEUR.md
/memory show         — affiche le contenu actuel
/memory clear        — vide ORCHESTRATEUR.md
```

---

## Routing automatique

```
Fichier / prompt analysé
        │
        ├─ .rs dans src-tauri/ ou mots-clés tauri ──→  HIGH   → Sonnet
        ├─ .rs / Cargo.*  ────────────────────────────→  HIGH   → Sonnet
        ├─ .vue / .jsx / .tsx / .html / .css  ────────→  MEDIUM → Haiku
        ├─ .ts / .js / package.json / vite.*  ────────→  MEDIUM → Haiku
        ├─ .py / .go  ────────────────────────────────→  MEDIUM → Haiku
        └─ .md / .json / .sh / .env  ─────────────────→  LOW    → Local

Clé absente → fallback automatique.
429 rate-limit → message + /retry pour basculer en local.
```

### Profils de coût (`/profile`)

| Profil | High | Medium | Low |
|---|---|---|---|
| `balanced` (défaut) | Sonnet | Haiku→Grok→Local | Local→Grok |
| `quality` | Sonnet | Sonnet | Haiku |
| `cheap` | Local | Local | Local |

---

## Commandes slash

### Tâches et session

| Commande | Action |
|---|---|
| `/plan <prompt>` | Affiche le plan et l'estimation de coût sans exécuter |
| `/status` | Tâches pending / done / failed |
| `/cost` | Coût total par modèle + économies vs tout-Sonnet |
| `/cancel` | Annule les tâches en attente |
| `/retry` | Relance en local les tâches rate-limitées (429) |
| `/clear` | Auto-résumé de session puis réinitialise le contexte |
| `/reset` | Repart de zéro (nouvelle session) |
| `/export` | Exporte le résumé de session en Markdown |
| `/resume` | Reprend après une pause Ctrl+C |

### Modèles et qualité

| Commande | Action |
|---|---|
| `/btw` | Ouvre l'overlay de sélection de modèle (1=Sonnet 2=Haiku 3=Gemini 0=auto) |
| `/btw <note>` | Ajoute une note de contexte sans déclencher de tâche |
| `/profile quality` / `balanced` / `cheap` | Change le profil de coût pour les prochaines tâches |
| `/autowrite on` / `off` | Active/désactive l'écriture automatique des fichiers |
| `/verbose` | Toggle le mode verbeux (logs des réponses LLM complètes) |

### Contexte et mémoire

| Commande | Action |
|---|---|
| `/add <chemin>` | Ajoute un fichier au contexte manuel de chaque appel LLM |
| `/context list` | Liste les fichiers en contexte manuel |
| `/context rm <chemin>` | Retire un fichier du contexte manuel |
| `/memory add <texte>` | Ajoute une note à ORCHESTRATEUR.md |
| `/memory show` | Affiche la mémoire projet courante |
| `/memory clear` | Vide ORCHESTRATEUR.md |

### Vues et navigation

| Commande | Action |
|---|---|
| `/log` | Passe en vue Logs |
| `/diff` | Passe en vue Diff (dernier fichier modifié) |
| `Tab` | Cycle Normal → Logs → Diff |
| `↑` / `↓` | Historique des prompts (200 entrées max) |

### Git

| Commande | Action |
|---|---|
| `/git status` | Affiche la branche et les fichiers modifiés |
| `/git commit` | Génère un message de commit via le LLM local |
| `/git log` | Affiche les 5 derniers commits |

### Diagnostic

| Commande | Action |
|---|---|
| `/doctor` | Diagnostic complet : clés API, modèle local, répertoire projet, cargo, git, profil |
| `/help` | Liste toutes les commandes |

---

## LLM local — llama-cpp-4 + Qwen2.5-Coder

Le LLM local tourne **directement en mémoire** via le binding Rust [`llama-cpp-4`](https://crates.io/crates/llama-cpp-4) — **aucun serveur HTTP, aucun daemon**.

### Sélection automatique selon la RAM

| RAM disponible | Modèle | Format |
|---|---|---|
| ≤ 8 Go | `Qwen2.5-Coder-3B-Instruct` | Q4_K_M |
| ≤ 24 Go | `Qwen2.5-Coder-7B-Instruct` | Q4_K_M |
| > 24 Go | `Qwen2.5-Coder-14B-Instruct` | Q4_K_M |

Téléchargement automatique depuis [`bartowski`](https://huggingface.co/bartowski) au premier lancement. Les suivants utilisent le cache `~/.cache/huggingface/hub/` — le TUI affiche `loading…` au lieu de `downloading…`.

### Accélération GPU

| Plateforme | Backend |
|---|---|
| macOS (Apple Silicon / Intel) | **Metal** — toutes les couches en GPU par défaut |
| Linux / Windows | CPU (CUDA optionnel via feature flag) |

### Forcer un modèle spécifique

```bash
# Dans ~/.config/orchestrateur-llm/.env
MODEL_REPO=bartowski/Qwen2.5-Coder-3B-Instruct-GGUF
MODEL_FILE=Qwen2.5-Coder-3B-Instruct-Q4_K_M.gguf
```

---

## Prérequis

- **Rust** ≥ 1.75 — [rustup.rs](https://rustup.rs)
- **macOS** : Xcode Command Line Tools (`xcode-select --install`)
- **Linux** : `build-essential cmake`
- Au moins une clé API parmi Anthropic, Gemini, Grok, OpenAI (toutes optionnelles — repli sur local)
- Connexion internet au premier lancement (téléchargement du modèle HuggingFace)

---

## Installation

```bash
git clone https://github.com/mbougrin/orchestrateur-llm
cd orchestrateur-llm
./install.sh
```

Le script :
1. Compile le binaire en mode `release` (avec Metal sur macOS)
2. Installe `orchestrateur-llm` dans `/usr/local/bin`
3. Crée `~/.config/orchestrateur-llm/config.toml`
4. Crée `~/.config/orchestrateur-llm/.env` (chmod 600)
5. Ajoute le source automatique dans votre shell RC

---

## Configuration des clés API

```bash
nano ~/.config/orchestrateur-llm/.env
```

```bash
# Anthropic (Claude Sonnet + Haiku)
ANTHROPIC_API_KEY=sk-ant-...

# Google Gemini Flash
GEMINI_API_KEY=AIza-...

# xAI Grok (grok-3-mini)
GROK_API_KEY=xai-...

# OpenAI GPT (gpt-4o-mini)
OPENAI_API_KEY=sk-...
```

```bash
source ~/.config/orchestrateur-llm/.env
```

Toutes les clés sont optionnelles — l'orchestrateur utilise ce qui est disponible et fallback automatiquement.

---

## Configuration de l'architecture LLM

`~/.config/orchestrateur-llm/config.toml` — section `[llm_arch]` :

```toml
[llm_arch.high]
preferred = "sonnet"
fallbacks = ["gemini", "grok"]

[llm_arch.medium]
preferred = "haiku"
fallbacks = ["grok", "gemini", "local"]

[llm_arch.low]
preferred = "local"
fallbacks = ["grok"]
```

Noms acceptés : `"sonnet"`, `"haiku"`, `"gemini"`, `"grok"`, `"gpt"` / `"openai"`, `"local"`.

---

## Variables d'environnement

| Variable | Défaut | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | — | Clé API Anthropic (Claude Sonnet + Haiku) |
| `GEMINI_API_KEY` | — | Clé API Google Gemini Flash |
| `GROK_API_KEY` | — | Clé API xAI Grok (grok-3-mini) |
| `OPENAI_API_KEY` | — | Clé API OpenAI (gpt-4o-mini) |
| `MODEL_REPO` | auto RAM | Dépôt HuggingFace du modèle GGUF local |
| `MODEL_FILE` | auto RAM | Fichier GGUF à télécharger |
| `LLM_TEMPERATURE` | `0.2` | Température d'inférence locale |
| `LLM_N_CTX` | auto RAM | Fenêtre de contexte (tokens) |
| `LLM_N_GPU_LAYERS` | `999` macOS | Couches déchargées sur GPU Metal |
| `LLM_N_THREADS` | auto CPU | Nombre de threads CPU |

---

## Exemples de prompts

```
Ajoute un endpoint REST /api/users en Rust
```
→ RustAgent (Sonnet, High) → BuilderAgent crée/patch les fichiers → VerifyAgent (`cargo check`)

```
Crée le composant UserList.vue + mets à jour le README
```
→ FrontendAgent (Haiku, Medium) + DocAgent (Gemini, Low) **en parallèle**

```
Fix typo in config.rs
```
→ LocalAgent (Qwen2.5, Low) — **0 token API**

```
Crée src/api/users.rs + src/api/mod.rs + tests/api_test.rs
```
→ Un seul appel LLM — BuilderAgent écrit les 3 fichiers en une passe

```
/profile cheap
Refactorise tout le module auth
```
→ Force le routage sur Local — coût zéro

---

## Développement

```bash
# Vérifier tout le workspace
cargo check --workspace

# Lancer en mode debug (logs sur stderr)
cargo run -p tui -- --debug

# Compiler en release
cargo build --release -p tui
./target/release/orchestrateur-llm
```

Logs : `~/.local/share/orchestrateur-llm/orchestrateur-llm.log`

---

## Licence

MIT
