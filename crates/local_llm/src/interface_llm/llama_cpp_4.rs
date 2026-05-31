use anyhow::Error;
use llama_cpp_4::{
    llama_backend::LlamaBackend,
    model::LlamaModel,
};
use llama_cpp_sys_4::ggml_log_level;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::interface_llm::config_llm::get_config_model_params;
use crate::interface_llm::rag::{request_llm, request_rag_llm, request_rag_llm_with_temp, request_rag_llm_streaming};

pub static MODEL_LOADED:  Mutex<Option<Arc<LlamaModel>>>   = Mutex::new(None);
pub static MODEL_BACKEND: Mutex<Option<Arc<LlamaBackend>>> = Mutex::new(None);

pub fn close_llm() {
    tracing::info!("--- 🔄 Fermeture du LLM ---");
    {
        let mut guard = MODEL_LOADED.lock().unwrap();
        if guard.is_some() {
            *guard = None;
            tracing::info!("  ✅ Modèle déchargé de la mémoire");
        }
    }
    {
        let mut guard = MODEL_BACKEND.lock().unwrap();
        if guard.is_some() {
            *guard = None;
            tracing::info!("  ✅ Backend Llama arrêté");
        }
    }
    tracing::info!("--- 📦 Ressources libérées avec succès ---");
}

pub fn load_model(path_model: String) -> bool {
    // Suppress ALL llama.cpp C-level stderr output before any init call.
    // LlamaBackend::init() calls llama_backend_init() which can print Metal
    // initialisation lines; void_logs() only works after that, so we install
    // the no-op callback here first, before init.
    unsafe extern "C" fn void_log(
        _level: ggml_log_level,
        _text: *const std::os::raw::c_char,
        _user_data: *mut std::os::raw::c_void,
    ) {}
    unsafe { llama_cpp_sys_4::llama_log_set(Some(void_log), std::ptr::null_mut()); }

    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => { tracing::error!("Erreur LlamaBackend : {}", e); return false; }
    };

    tracing::info!("--- Loading Model ---");
    tracing::info!("Model Path: {}", path_model);
    let start = Instant::now();

    let model = match LlamaModel::load_from_file(&backend, path_model, &get_config_model_params()) {
        Ok(m) => m,
        Err(e) => { tracing::error!("Erreur LlamaModel : {}", e); return false; }
    };

    tracing::info!("Time Loaded Model: {:?}", start.elapsed());
    tracing::info!("--- Model Loaded ---");

    if let Ok(mut guard) = MODEL_LOADED.lock()  { *guard = Some(Arc::new(model));   } else { return false; }
    if let Ok(mut guard) = MODEL_BACKEND.lock() { *guard = Some(Arc::new(backend)); } else { return false; }
    true
}

fn check_model_loaded() -> Result<(), Error> {
    if MODEL_LOADED.lock().unwrap().is_none() {
        tracing::error!("❌ [ERROR] LLM Core: modèle non chargé.");
        return Err(Error::msg("Modèle non chargé en mémoire."));
    }
    Ok(())
}

pub fn run_llm_request(prompt: String) -> Result<String, Error> {
    check_model_loaded()?;
    tracing::info!("\n🚀 [LLM REQUEST] inference…");
    let start = Instant::now();
    let ret = request_llm(prompt.as_str());
    tracing::info!("⏱️  {:.2?}", start.elapsed());
    Ok(ret)
}

pub async fn run_llm_request_rag(prompt: String, rules: String) -> Result<String, Error> {
    run_llm_request_rag_inner(prompt, rules, None).await
}

pub async fn run_llm_request_rag_with_temp(prompt: String, rules: String, temp: f32) -> Result<String, Error> {
    run_llm_request_rag_inner(prompt, rules, Some(temp)).await
}

pub async fn run_llm_request_rag_streaming(
    prompt: String,
    rules: String,
    temp: f32,
    token_tx: tokio::sync::mpsc::Sender<String>,
) -> Result<String, Error> {
    check_model_loaded()?;
    tracing::info!("\n🚀 [LLM STREAM] {} chars prompt | temp={}", prompt.len(), temp);
    let start = Instant::now();
    let ret = tokio::task::spawn_blocking(move || {
        request_rag_llm_streaming(&rules, &prompt, temp, token_tx)
    }).await.map_err(|e| anyhow::anyhow!("LLM streaming task panicked: {}", e))?;
    tracing::info!("✅ [LLM STREAM] done in {:.2?}", start.elapsed());
    Ok(ret)
}

async fn run_llm_request_rag_inner(prompt: String, rules: String, temp: Option<f32>) -> Result<String, Error> {
    check_model_loaded()?;

    tracing::info!("\n🚀 [LLM] {} chars prompt | {} chars rules | temp={:?}",
        prompt.len(), rules.len(), temp);

    let start = Instant::now();

    let ret = tokio::task::spawn_blocking(move || {
        match temp {
            Some(t) => request_rag_llm_with_temp(&rules, &prompt, t),
            None    => request_rag_llm(&rules, &prompt),
        }
    }).await.map_err(|e| anyhow::anyhow!("LLM blocking task panicked: {}", e))?;

    tracing::info!("✅ [LLM] done in {:.2?}", start.elapsed());
    Ok(ret)
}
