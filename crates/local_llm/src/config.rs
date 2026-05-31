use std::env;

pub struct AppConfig {
    pub model_repo: Option<String>,
    pub model_file: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            model_repo: env::var("MODEL_REPO").ok().filter(|s| !s.is_empty()),
            model_file: env::var("MODEL_FILE").ok().filter(|s| !s.is_empty()),
        }
    }
}

pub fn llm_temperature() -> f32 {
    env::var("LLM_TEMPERATURE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.2)
}

pub fn llm_top_k() -> i32 {
    env::var("LLM_TOP_K").ok().and_then(|v| v.parse().ok()).unwrap_or(40)
}

pub fn llm_top_p() -> f32 {
    env::var("LLM_TOP_P").ok().and_then(|v| v.parse().ok()).unwrap_or(0.90)
}

pub fn llm_seed() -> u32 {
    env::var("LLM_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(42)
}

pub fn llm_max_new_tokens() -> usize {
    env::var("LLM_MAX_NEW_TOKENS").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub fn llm_n_ctx() -> Option<u32> {
    env::var("LLM_N_CTX").ok().and_then(|v| v.parse::<u32>().ok()).filter(|&n| n > 0)
}

pub fn llm_n_gpu_layers() -> Option<u32> {
    env::var("LLM_N_GPU_LAYERS")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|v| v.parse::<u32>().ok())
}

pub fn llm_n_threads() -> Option<i32> {
    env::var("LLM_N_THREADS").ok().and_then(|v| v.parse::<i32>().ok()).filter(|&n| n > 0)
}
