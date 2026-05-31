#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
#  orchestrateur-llm — Install script
#  macOS & Linux
# ─────────────────────────────────────────────

BINARY_NAME="orchestrateur-llm"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="${HOME}/.config/orchestrateur-llm"
DATA_DIR="${HOME}/.local/share/orchestrateur-llm"
ENV_FILE="${CONFIG_DIR}/.env"
CONFIG_FILE="${CONFIG_DIR}/config.toml"

# ── Colors ────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[info]${RESET}  $*"; }
success() { echo -e "${GREEN}[ok]${RESET}    $*"; }
warn()    { echo -e "${YELLOW}[warn]${RESET}  $*"; }
error()   { echo -e "${RED}[error]${RESET} $*" >&2; }
step()    { echo -e "\n${BOLD}▶ $*${RESET}"; }

# ── Detect OS ─────────────────────────────────
detect_os() {
    case "$(uname -s)" in
        Darwin) echo "macos" ;;
        Linux)  echo "linux" ;;
        *)      error "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac
}

OS=$(detect_os)
info "Detected OS: ${OS}"

# ── Check dependencies ────────────────────────
step "Checking dependencies"

if ! command -v cargo &>/dev/null; then
    error "Rust/Cargo not found. Install from https://rustup.rs"
    exit 1
fi
success "cargo $(cargo --version)"

if ! command -v git &>/dev/null; then
    error "git not found."
    exit 1
fi
success "git $(git --version)"

if [ "$OS" = "macos" ]; then
    if ! xcode-select -p &>/dev/null; then
        warn "Xcode command line tools not found. Installing..."
        xcode-select --install
    else
        success "Xcode CLT: $(xcode-select -p)"
    fi
fi

if [ "$OS" = "linux" ]; then
    for pkg in gcc cmake; do
        if ! command -v "$pkg" &>/dev/null; then
            warn "$pkg not found. Trying to install..."
            if command -v apt-get &>/dev/null; then
                sudo apt-get install -y build-essential cmake
            elif command -v dnf &>/dev/null; then
                sudo dnf install -y gcc cmake
            else
                error "Cannot install $pkg automatically. Please install it manually."
                exit 1
            fi
        fi
    done
    success "Build tools OK"
fi

# ── Build ─────────────────────────────────────
step "Building orchestrateur-llm (release)"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
info "Source: ${SCRIPT_DIR}"

# macOS: dylibs from llama-cpp-4 (Metal) are installed here so dyld can find them.
DYLIB_DIR="${HOME}/.local/lib/orchestrateur-llm"

if [ "$OS" = "macos" ]; then
    info "Building with Metal support..."
else
    info "Building (CPU mode)..."
fi

cargo build --release -p tui 2>&1 | tail -5

BUILT_BINARY="${SCRIPT_DIR}/target/release/${BINARY_NAME}"

if [ ! -f "$BUILT_BINARY" ]; then
    error "Build failed — binary not found at ${BUILT_BINARY}"
    exit 1
fi
success "Built: ${BUILT_BINARY}"

# ── Install dylibs (macOS only) ───────────────
if [ "$OS" = "macos" ]; then
    step "Installing Metal dylibs to ${DYLIB_DIR}"
    mkdir -p "${DYLIB_DIR}"
    find "${SCRIPT_DIR}/target/release" -maxdepth 1 -name "*.dylib" \
        -exec cp {} "${DYLIB_DIR}/" \;
    # Embed the rpath so dyld finds the libs wherever the binary is installed.
    install_name_tool -add_rpath "${DYLIB_DIR}" "${BUILT_BINARY}" 2>/dev/null || true
    success "Dylibs → ${DYLIB_DIR}"
fi

# ── Install binary ────────────────────────────
step "Installing binary"

do_install() {
    local dest_dir="$1"
    if [ ! -w "$dest_dir" ]; then
        sudo install -m 755 "$BUILT_BINARY" "${dest_dir}/${BINARY_NAME}"
    else
        install -m 755 "$BUILT_BINARY" "${dest_dir}/${BINARY_NAME}"
    fi
    success "Installed: ${dest_dir}/${BINARY_NAME}"
}

# Replace every existing copy found in PATH to avoid stale binaries taking precedence.
INSTALLED_ANY=false
IFS=':' read -ra PATH_DIRS <<< "$PATH"
for dir in "${PATH_DIRS[@]}"; do
    if [ -f "${dir}/${BINARY_NAME}" ]; then
        info "Replacing existing binary in ${dir}"
        do_install "$dir"
        INSTALLED_ANY=true
    fi
done

# Always install to INSTALL_DIR as the canonical location.
do_install "$INSTALL_DIR"
INSTALLED_ANY=true

# ── Create directories ────────────────────────
step "Setting up directories"

mkdir -p "$CONFIG_DIR"
mkdir -p "$DATA_DIR"
mkdir -p "${DATA_DIR}/cache"
success "Config : ${CONFIG_DIR}"
success "Data   : ${DATA_DIR}"

# ── Deploy config.toml + .env (ensemble) ──────
step "Deploying config.toml and .env"

# config.toml contains no secrets — always overwrite to keep it in sync with the binary.
# .env contains API keys — preserve it if it already exists.
DEPLOY_ENV=true

if [ -f "$ENV_FILE" ]; then
    warn ".env already exists — skipping (API keys preserved)"
    DEPLOY_ENV=false
fi

cat > "$CONFIG_FILE" << 'TOML'
[models]
rust_agent     = "claude-sonnet-4-5"
frontend_agent = "claude-haiku-4-5"
doc_agent      = "gemini-2.0-flash"

[local_llm]
# Téléchargement automatique depuis HuggingFace au premier lancement.
# Pour forcer un modèle, définir MODEL_REPO + MODEL_FILE dans .env.
enabled              = true
confidence_threshold = 0.85

[context]
max_history_messages     = 20
auto_summarize_threshold = 10000
summary_model            = "claude-haiku"

[build_validation]
enabled        = true
rust_command   = "cargo check"
node_command   = "npm run type-check"
python_command = "python -m py_compile"
go_command     = "go build ./..."
max_retries    = 3

[parallelism]
max_concurrent_agents = 4
agent_timeout_secs    = 120

[cache]
enabled     = true
ttl_hours   = 24
max_size_mb = 100

[tui]
refresh_rate_ms   = 100
show_logs_default = false
theme             = "dark"

[cost_alert]
warn_at_usd = 1.0
stop_at_usd = 5.0

# LLM architecture — which model handles each priority tier.
# Fallbacks are tried in order when the preferred model key is absent.
[llm_arch.high]
preferred = "sonnet"
fallbacks = ["gemini", "grok"]

[llm_arch.medium]
preferred = "haiku"
fallbacks = ["grok", "gemini", "local"]

[llm_arch.low]
preferred = "local"
fallbacks = ["grok"]
TOML
success "config.toml → ${CONFIG_FILE}"

if [ "$DEPLOY_ENV" = true ]; then
    cat > "$ENV_FILE" << 'ENV'
# orchestrateur-llm — API Keys
# Remplissez vos clés puis rechargez : source ~/.config/orchestrateur-llm/.env

# Anthropic (Claude Sonnet + Haiku)
ANTHROPIC_API_KEY=sk-ant-REPLACE_ME

# Google Gemini Flash
GEMINI_API_KEY=AIza-REPLACE_ME

# xAI Grok (grok-3-mini)
GROK_API_KEY=xai-REPLACE_ME

# OpenAI GPT (gpt-4o-mini)
OPENAI_API_KEY=sk-REPLACE_ME

# Modèle local llama-cpp-4 (optionnel — détection auto selon la RAM)
# ≤  8 Go → Qwen2.5-Coder-3B   (Q4_K_M)
# ≤ 24 Go → Qwen2.5-Coder-7B   (Q4_K_M)
# > 24 Go → Qwen2.5-Coder-14B  (Q4_K_M)
# Pour forcer un modèle :
# MODEL_REPO=bartowski/Qwen2.5-Coder-3B-Instruct-GGUF
# MODEL_FILE=Qwen2.5-Coder-3B-Instruct-Q4_K_M.gguf

# Tuning inférence llama-cpp-4 (optionnel)
# LLM_TEMPERATURE=0.2
# LLM_TOP_K=40
# LLM_TOP_P=0.9
# LLM_SEED=42
# LLM_MAX_NEW_TOKENS=0
# LLM_N_CTX=8192
# LLM_N_GPU_LAYERS=999
# LLM_N_THREADS=
ENV
    chmod 600 "$ENV_FILE"
    success ".env       → ${ENV_FILE}"
fi

# ── Shell integration ─────────────────────────
step "Shell integration"

SHELL_RC=""
case "$SHELL" in
    */zsh)  SHELL_RC="${HOME}/.zshrc" ;;
    */bash) SHELL_RC="${HOME}/.bashrc" ;;
    */fish) SHELL_RC="${HOME}/.config/fish/config.fish" ;;
esac

MARKER="orchestrateur-llm/.env"
SOURCELINE="[ -f \"\$HOME/.config/orchestrateur-llm/.env\" ] && source \"\$HOME/.config/orchestrateur-llm/.env\""

if [ -n "$SHELL_RC" ] && ! grep -q "$MARKER" "$SHELL_RC" 2>/dev/null; then
    {
        echo ""
        echo "# orchestrateur-llm API keys"
        echo "$SOURCELINE"
    } >> "$SHELL_RC"
    success "Auto-source ajouté dans ${SHELL_RC}"
else
    info "Shell RC déjà configuré ou shell non reconnu"
fi

# ── Résumé ────────────────────────────────────
echo ""
echo -e "${BOLD}${GREEN}✓ orchestrateur-llm installé avec succès !${RESET}"
echo ""
echo -e "  Binaire  : ${CYAN}${INSTALL_DIR}/${BINARY_NAME}${RESET}"
echo -e "  Config   : ${CYAN}${CONFIG_FILE}${RESET}"
echo -e "  Clés API : ${CYAN}${ENV_FILE}${RESET}"
echo ""
echo -e "${YELLOW}Prochaines étapes :${RESET}"
echo -e "  1. Éditez vos clés API :"
echo -e "     ${CYAN}nano ${ENV_FILE}${RESET}"
echo -e "  2. Rechargez votre shell :"
echo -e "     ${CYAN}source ${ENV_FILE}${RESET}"
echo -e "  3. Lancez l'orchestrateur dans votre projet :"
echo -e "     ${CYAN}cd /votre/projet && orchestrateur-llm${RESET}"
echo ""
