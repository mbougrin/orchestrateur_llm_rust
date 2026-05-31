use hf_hub::{Repo, RepoType};
use std::path::PathBuf;
use hf_hub::api::sync::ApiBuilder;
use crate::config::AppConfig;
use crate::hardware::hardware_profile::HardwareProfile;

fn check_config_model() -> (String, String) {
    let cfg = AppConfig::from_env();

    if let (Some(repo), Some(file)) = (cfg.model_repo, cfg.model_file) {
        tracing::info!("[CONFIG] Model override from env: {}/{}", repo, file);
        return (repo, file);
    }

    let hw = HardwareProfile::detect();
    let ram = hw.total_ram_gb as i32;
    tracing::info!("[SYSTEM] RAM détectée : {} GB", ram);

    if ram <= 8 {
        tracing::info!("[CONFIG] Sélection du modèle 3B (Profil Basse Consommation)");
        (
            "bartowski/Qwen2.5-Coder-3B-Instruct-GGUF".to_string(),
            "Qwen2.5-Coder-3B-Instruct-Q4_K_M.gguf".to_string(),
        )
    } else if ram <= 24 {
        tracing::info!("[CONFIG] Sélection du modèle 7B (Profil Standard)");
        (
            "bartowski/Qwen2.5-Coder-7B-Instruct-GGUF".to_string(),
            "Qwen2.5-Coder-7B-Instruct-Q4_K_M.gguf".to_string(),
        )
    } else {
        tracing::info!("[CONFIG] Sélection du modèle 14B (Profil Haute Performance)");
        (
            "bartowski/Qwen2.5-Coder-14B-Instruct-GGUF".to_string(),
            "Qwen2.5-Coder-14B-Instruct-Q4_K_M.gguf".to_string(),
        )
    }
}

/// Returns true when the GGUF model file is already present in the hf-hub on-disk cache.
/// A blob > 500 MB is a strong indicator that the model was fully downloaded.
pub fn is_model_file_in_cache() -> bool {
    let (repo_id, _) = check_config_model();
    // hf-hub stores blobs at ~/.cache/huggingface/hub/models--{org}--{repo}/blobs/
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".cache/huggingface/hub")
        .join(format!("models--{}", repo_id.replace('/', "--")))
        .join("blobs");

    let Ok(entries) = std::fs::read_dir(&cache_dir) else { return false; };
    const MIN_GGUF_BYTES: u64 = 500 * 1024 * 1024; // 500 MB
    entries
        .filter_map(|e| e.ok())
        .any(|e| e.metadata().map(|m| m.len() > MIN_GGUF_BYTES).unwrap_or(false))
}

pub fn download_model_hugging_face() -> Result<PathBuf, String> {
    let (repo_id, file_name) = check_config_model();

    tracing::info!("[HF] Tentative de récupération : {}/{}", repo_id, file_name);

    // with_progress(false) — suppresses the stderr progress bar that corrupts TUI display
    let api = ApiBuilder::new()
        .with_progress(false)
        .build()
        .map_err(|e| format!("Erreur API HF : {}", e))?;

    let repo_obj = api.repo(Repo::new(repo_id, RepoType::Model));

    let path = repo_obj
        .get(&file_name)
        .map_err(|e| format!("Erreur lors du téléchargement : {}", e))?;

    tracing::info!("--- ✅ Modèle prêt ---");
    tracing::info!("📍 Chemin : {}", path.display());
    tracing::info!("----------------------");

    Ok(path)
}
