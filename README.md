# orchestrateur-llm

> Orchestrateur CLI/TUI qui divise automatiquement les tâches de développement entre plusieurs LLM pour minimiser les coûts en tokens tout en maximisant la qualité du code produit.

**Principe** : chaque tâche est classifiée par priorité (High / Medium / Low) et routée vers le LLM le plus adapté. Le modèle local tourne **en mémoire sans serveur** via `llama-cpp-4`.

---

## Modèles et coûts

| Priorité | Modèle | Cas d'usage | Coût |
|---|---|---|---|
| **High** | `claude-sonnet-4-5` | Rust, Tauri, planification, build fix | $3 / 1M tokens |
| **Medium** | `claude-haiku-4-5` | Vue.js, HTML, TypeScript, CSS, Tauri frontend | $0.25 / 1M tokens |
| **Medium** | `grok-3-mini` | Fallback Haiku — OpenAI-compatible | $0.30 / 1M tokens |
| **Medium / High** | `gemini-2.0-flash` | Fallback Sonnet / Docs, scripts | ~gratuit (free tier) |
| **Medium / High** | `gpt-4o-mini` | Fallback GPT — OpenAI | $0.15 / 1M tokens |
| **Low** | `qwen2.5-coder` (local) | .md, .json, .sh, boilerplate, typos | **100% gratuit** |

### Chaîne de fallback par priorité

```
High   : Sonnet  → Gemini  → Grok
Medium : Haiku   → Grok    → Gemini → Local
Low    : Local   → Grok
```

La chaîne est configurable dans `~/.config/orchestrateur-llm/config.toml`.

---

## Interface

```
┌────────────────────────────────────────────────────────────────┐
│  orchestrateur-llm v1.0  [project: my-app]         [ESC: quit] │
├──────────────────────┬─────────────────────┬───────────────────┤
│  AGENTS ACTIFS       │  TOKEN USAGE        │  STATUS           │
│                      │                     │                   │
│  [⠹] RustAgent       │  Sonnet  ████░  1,2k│  Anthropic [✓ ok] │
│      claude-sonnet   │  Haiku   ██░░░   456│  Gemini    [✓ ok] │
│      "impl Display"  │  Grok    █░░░░    89│  Grok      [✓ ok] │
│                      │  GPT     ░░░░░     0│  GPT       [✓ ok] │
│  [⠙] DocAgent        │  Local   ──── FREE  │  Local  [ready]   │
│      gemini-flash    │                     │                   │
│      "update README" │  Total: ~$0.0142    │  CPU  [████░░░░]  │
│                      │  Saved: ~73%        │  RAM  [███░░░░░]  │
│                      │                     │  Swap [░░░░░░░░]  │
├──────────────────────┴─────────────────────┴───────────────────┤
│  QUEUE DES TÂCHES                                              │
│  [✓] Analyze project structure                                 │
│  [✓] Plan tasks (orchestrator)                                 │
│  [►] impl Display for Config  [HIGH → sonnet]                  │
│  [►] Update README with new API  [LOW → gemini]                │
│  [○] Build validation  [MEDIUM → local]                        │
├────────────────────────────────────────────────────────────────┤
│  > █                                                           │
│  /btw /clear /status /cost /plan /cancel /retry /log /help     │
└────────────────────────────────────────────────────────────────┘
```

---

## Architecture des crates

```
crates/
├── core/
│   ├── task.rs            Task · TaskType · TaskStatus · Priority (High/Medium/Low)
│   ├── orchestrator.rs    Orchestrator — file du dispatcher
│   ├── dispatcher.rs      Routing par priorité + disponibilité des clés API
│   ├── llm_arch_config.rs LlmArchConfig — lu depuis config.toml
│   └── context.rs         AppContext (clés API, LlmArchConfig, compteurs tokens)
│
├── agents/
│   ├── rust_agent.rs      RustAgent      → Sonnet (code Rust / Tauri)
│   ├── frontend_agent.rs  FrontendAgent  → Haiku  (Vue, TS, CSS)
│   ├── doc_agent.rs       DocAgent       → Gemini  (Markdown, scripts)
│   ├── grok_agent.rs      GrokAgent      → Grok    (xAI, OpenAI-compatible)
│   ├── gpt_agent.rs       GptAgent       → GPT-4o-mini (OpenAI)
│   ├── local_agent.rs     LocalAgent     → llama-cpp-4 in-process
│   ├── builder_agent.rs   BuilderAgent   — parse la sortie LLM et crée les fichiers
│   ├── build_agent.rs     BuildValidator — cargo check / npm type-check / go build
│   └── verify_agent.rs    VerifyAgent    — lance BuildValidator, renvoie erreurs au coder
│
├── llm_clients/
│   ├── anthropic.rs       Claude Sonnet + Haiku  (détection 429 → "rate_limit:")
│   ├── gemini.rs          Gemini Flash           (détection 429 → "rate_limit:")
│   ├── grok.rs            Grok-3-mini  xAI       (détection 429 → "rate_limit:")
│   └── gpt.rs             GPT-4o-mini  OpenAI    (détection 429 → "rate_limit:")
│
├── local_llm/
│   ├── config.rs              Variables d'env LLM (température, n_ctx, gpu_layers…)
│   ├── hardware/              Détection RAM/CPU (macOS · Linux · Windows)
│   ├── models_download/       Téléchargement HuggingFace (hf-hub) + détection cache
│   └── interface_llm/         Inférence llama-cpp-4 (load · RAG · streaming)
│
├── file_analyzer/         Scanner walkdir · Chunker · Classifier (TauriRust, Frontend…)
├── context_store/         Persistance SQLite
└── tui/                   Interface Ratatui (binaire orchestrateur-llm)
```

---

## Routing automatique

```
Fichier reçu / prompt analysé
        │
        ├─ .rs dans src-tauri/  ──────────────→  HIGH   → Sonnet
        ├─ tauri.conf.json / mots-clés tauri ─→  HIGH   → Sonnet
        ├─ .rs / Cargo.*  ────────────────────→  HIGH   → Sonnet
        ├─ .vue / .jsx / .tsx / .html / .css ─→  MEDIUM → Haiku
        ├─ .ts / .js / package.json / vite.* ─→  MEDIUM → Haiku
        ├─ .py / .go  ────────────────────────→  MEDIUM → Haiku
        └─ .md / .json / .sh / .env  ─────────→  LOW    → Local

        Si la clé du modèle préféré est absente, la chaîne de fallback s'active.
        Si rate-limit (429) → message utilisateur + /retry pour basculer en local.
```

### BuilderAgent + VerifyAgent

Après chaque agent codeur :

```
Agent coder (Sonnet/Haiku/Grok/GPT)
        │  sortie LLM avec blocs annotés :
        │  ```rust:src/main.rs
        ▼
BuilderAgent.extract_files()  → crée les fichiers sur disque
        │
        ▼
VerifyAgent (BuildValidator)
        │
        ├─ Build OK  ──→ tâche marquée Done
        │
        └─ Build FAIL ──→ erreurs renvoyées au coder (retry, max 3)
```

---

## LLM local — llama-cpp-4 + Qwen2.5-Coder

Le LLM local tourne **directement en mémoire** via le binding Rust [`llama-cpp-4`](https://crates.io/crates/llama-cpp-4) — **aucun serveur HTTP, aucun daemon**.

### Modèles (sélection automatique selon la RAM)

| RAM | Modèle | Format |
|---|---|---|
| ≤ 8 Go | `Qwen2.5-Coder-3B-Instruct` | Q4_K_M |
| ≤ 24 Go | `Qwen2.5-Coder-7B-Instruct` | Q4_K_M |
| > 24 Go | `Qwen2.5-Coder-14B-Instruct` | Q4_K_M |

Téléchargement automatique depuis [`bartowski`](https://huggingface.co/bartowski) sur HuggingFace au premier lancement. Les lancements suivants utilisent le cache local (`~/.cache/huggingface/hub/`) — le TUI affiche `loading…` au lieu de `downloading…`.

### Accélération GPU

| Plateforme | Backend |
|---|---|
| macOS (Apple Silicon / Intel) | **Metal** (toutes les couches en GPU par défaut) |
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
- Clés API : au moins une parmi Anthropic, Gemini, Grok, OpenAI (toutes optionnelles — repli sur local)
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

Noms de modèles acceptés : `"sonnet"`, `"haiku"`, `"gemini"`, `"grok"`, `"gpt"` / `"openai"`, `"local"`.

---

## Utilisation

```bash
cd /chemin/vers/mon-projet
orchestrateur-llm
```

### Exemples de prompts

```
Ajoute un endpoint REST /api/users en Rust
```
→ Planning → RustAgent (Sonnet, High) → BuilderAgent crée les fichiers → VerifyAgent (`cargo check`)

```
Crée le composant UserList.vue + mets à jour le README
```
→ FrontendAgent (Haiku, Medium) pour le .vue, DocAgent (Gemini, Low) pour le README, **en parallèle**

```
Fix typo in config.rs
```
→ LocalAgent (Qwen2.5, Low) — **0 token API**

```
[rate-limit Gemini] quota API dépassé
```
→ Tapez `/retry` pour relancer les tâches échouées en local automatiquement

---

## Commandes slash

| Commande | Action |
|---|---|
| `/plan <prompt>` | Affiche le plan de tâches et l'estimation de coût sans exécuter |
| `/cost` | Détail des coûts par modèle + économies vs tout-Sonnet |
| `/status` | Nombre de tâches pending / done / failed |
| `/cancel` | Annule les tâches en attente |
| `/retry` | Relance en local les tâches échouées sur rate-limit (429) |
| `/clear` | Vide le contexte de la session courante |
| `/reset` | Repart de zéro (nouvelle session) |
| `/log` | Toggle le panneau de logs détaillés (`Tab` aussi) |
| `/btw <note>` | Ajoute une note de contexte sans déclencher de tâche |
| `/export` | Exporte le résumé de session en Markdown |
| `/help` | Liste toutes les commandes |

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

Logs en mode normal : `~/.local/share/orchestrateur-llm/orchestrateur-llm.log`

---

## Licence

MIT
