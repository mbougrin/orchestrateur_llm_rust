pub mod config;
pub mod hardware;
pub mod models_download;
pub mod interface_llm;

use std::path::PathBuf;

/// Returns true if the model is already loaded in memory.
pub fn is_model_loaded() -> bool {
    interface_llm::llama_cpp_4::MODEL_LOADED
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

/// Downloads the model from HuggingFace (if not cached) then loads it via llama-cpp-4.
/// No-op if the model is already loaded.
pub fn ensure_model_loaded() -> Result<(), String> {
    if is_model_loaded() {
        return Ok(());
    }
    let path = models_download::models_download::download_model_hugging_face()?;
    let path_str = path.to_str().ok_or("Invalid model path")?.to_string();
    if interface_llm::llama_cpp_4::load_model(path_str) {
        Ok(())
    } else {
        Err("llama-cpp-4: failed to load model".to_string())
    }
}

/// Load a model from an explicit local path (skip HuggingFace download).
pub fn load_model_from_path(path: &PathBuf) -> Result<(), String> {
    if is_model_loaded() {
        return Ok(());
    }
    let path_str = path.to_str().ok_or("Invalid model path")?.to_string();
    if interface_llm::llama_cpp_4::load_model(path_str) {
        Ok(())
    } else {
        Err("llama-cpp-4: failed to load model from path".to_string())
    }
}

pub use interface_llm::llama_cpp_4::{
    close_llm,
    run_llm_request,
    run_llm_request_rag,
    run_llm_request_rag_with_temp,
    run_llm_request_rag_streaming,
};

pub use models_download::models_download::download_model_hugging_face;
pub use models_download::models_download::is_model_file_in_cache;
