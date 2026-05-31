use llama_cpp_4::llama_batch::LlamaBatch;
use llama_cpp_4::model::{AddBos, Special};
use llama_cpp_4::token::LlamaToken;
use crate::interface_llm::config_llm::{get_ctx_params_config, get_ctx_params_config_embed, get_sample_config_with_temp};

const MAX_EMBED_TOKENS: usize = 512;
const BATCH_SIZE: usize = 512;

fn embed_text(
    ctx: &mut llama_cpp_4::context::LlamaContext,
    model: &llama_cpp_4::model::LlamaModel,
    text: &str,
) -> Vec<f32> {
    let mut tokens = model
        .str_to_token(text, AddBos::Always)
        .map_err(|e| tracing::error!("[EMBED_TEXT][ERROR] Tokenisation échouée: {:?}", e))
        .expect("Tokenisation échouée");

    tokens.truncate(MAX_EMBED_TOKENS);

    if tokens.is_empty() {
        tracing::warn!("[EMBED_TEXT] Tokens vides");
        return vec![0.0; 4096];
    }

    let mut batch = LlamaBatch::new(MAX_EMBED_TOKENS, 1);
    for (i, &tok) in tokens.iter().enumerate() {
        batch
            .add(tok, i as i32, &[0], i == tokens.len() - 1)
            .expect("Erreur ajout token batch");
    }

    ctx.clear_kv_cache();
    ctx.decode(&mut batch).expect("Erreur decode embedding");

    ctx.embeddings_seq_ith(0).expect("Erreur extraction embedding").to_vec()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let dot: f32 = a[..len].iter().zip(b[..len].iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a[..len].iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b[..len].iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
}

fn run_generation(
    gen_ctx: &mut llama_cpp_4::context::LlamaContext,
    model: &llama_cpp_4::model::LlamaModel,
    prompt_tokens: &[LlamaToken],
    max_new_tokens: i32,
    n_batch: usize,
    temp: f32,
    token_tx: Option<&tokio::sync::mpsc::Sender<String>>,
) -> String {
    const STREAM_BATCH: usize = 8;

    if prompt_tokens.is_empty() || max_new_tokens <= 0 {
        return String::new();
    }

    for (batch_idx, token_chunk) in prompt_tokens.chunks(n_batch).enumerate() {
        let offset = batch_idx * n_batch;
        let is_last_chunk = offset + token_chunk.len() >= prompt_tokens.len();

        let mut batch = LlamaBatch::new(n_batch, 1);
        for (i, &tok) in token_chunk.iter().enumerate() {
            let is_last_token = is_last_chunk && i == token_chunk.len() - 1;
            batch.add(tok, (offset + i) as i32, &[0], is_last_token).expect("Erreur batch prompt");
        }

        if let Err(e) = gen_ctx.decode(&mut batch) {
            tracing::error!("[RUN_GENERATION] Erreur decode batch {}: {}", batch_idx, e);
            return "Erreur: decode du prompt échoué.".to_string();
        }
    }

    let mut sampler = get_sample_config_with_temp(temp);
    let mut token_strings: Vec<String> = Vec::new();
    let mut stream_batch:  Vec<String> = Vec::new();
    let mut n_cur = prompt_tokens.len() as i32;
    let n_max = n_cur + max_new_tokens;

    loop {
        let token = sampler.sample(gen_ctx, -1);
        sampler.accept(token);

        if model.is_eog_token(token) || n_cur >= n_max {
            break;
        }

        let tok_str = model.token_to_str(token, Special::Tokenize).unwrap_or_default();

        if let Some(tx) = token_tx {
            stream_batch.push(tok_str.clone());
            if stream_batch.len() >= STREAM_BATCH {
                tx.blocking_send(stream_batch.join("")).ok();
                stream_batch.clear();
            }
        }

        token_strings.push(tok_str);
        n_cur += 1;

        let mut next_batch = LlamaBatch::new(1, 1);
        if let Err(e) = next_batch.add(token, n_cur - 1, &[0], true) {
            tracing::error!("[RUN_GENERATION][ERROR] Erreur ajout token suivant: {:?}", e);
            break;
        }
        if let Err(e) = gen_ctx.decode(&mut next_batch) {
            tracing::warn!("[RUN_GENERATION] Erreur decode token {}: {}", n_cur, e);
            break;
        }
    }

    if let Some(tx) = token_tx {
        if !stream_batch.is_empty() {
            tx.blocking_send(stream_batch.join("")).ok();
        }
    }

    token_strings.join("").trim().to_string()
}

pub fn chunk_rules(rules: &str, chunk_lines: usize, max_chunk_chars: usize) -> Vec<String> {
    rules
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .chunks(chunk_lines)
        .map(|c| c.join("\n").chars().take(max_chunk_chars * 3).collect::<String>())
        .collect()
}

pub fn request_llm(prompt: &str) -> String {
    let model = {
        let g = crate::interface_llm::llama_cpp_4::MODEL_LOADED.lock().unwrap();
        match g.as_ref() {
            Some(arc) => std::sync::Arc::clone(arc),
            None => { tracing::error!("[REQUEST_LLM] Modèle non chargé."); return "Erreur: Modèle non chargé.".to_string(); }
        }
    };
    let backend = {
        let g = crate::interface_llm::llama_cpp_4::MODEL_BACKEND.lock().unwrap();
        match g.as_ref() {
            Some(arc) => std::sync::Arc::clone(arc),
            None => { tracing::error!("[REQUEST_LLM] Backend non initialisé."); return "Erreur: Backend non initialisé.".to_string(); }
        }
    };

    const DEFAULT_MAX_NEW_TOKENS: usize = 2048;
    const MIN_NEW_TOKENS: usize = 128;

    let env_cap = crate::config::llm_max_new_tokens();
    let max_new_tokens_cap = if env_cap > 0 { env_cap } else { DEFAULT_MAX_NEW_TOKENS };

    let (ctx_params_gen, n_ctx, n_batch) = get_ctx_params_config();

    let prompt = format!(
        "<s><|im_start|>system\n<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant",
        prompt
    );

    let mut gen_ctx = match model.new_context(&backend, ctx_params_gen) {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!("[REQUEST_LLM] Erreur contexte génération: {}", e);
            return "Erreur: impossible de créer le contexte de génération.".to_string();
        }
    };

    let mut prompt_tokens = match model.str_to_token(&prompt, AddBos::Never) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("[REQUEST_LLM] Erreur tokenisation: {}", e);
            return "Erreur: tokenisation échouée.".to_string();
        }
    };

    let max_prompt_tokens = (n_ctx as usize).saturating_sub(MIN_NEW_TOKENS + 64).max(64);
    if prompt_tokens.len() > max_prompt_tokens {
        tracing::warn!("[REQUEST_LLM] Prompt trop long ({} tokens), troncature à {}", prompt_tokens.len(), max_prompt_tokens);
        prompt_tokens.truncate(max_prompt_tokens);
    }

    let prompt_len = prompt_tokens.len();
    let available  = (n_ctx as usize).saturating_sub(prompt_len + 64);
    let actual_max = max_new_tokens_cap.min(available).max(MIN_NEW_TOKENS) as i32;

    tracing::info!("[REQUEST_LLM] Prompt: {} tokens | Génération max: {} tokens", prompt_len, actual_max);

    run_generation(&mut gen_ctx, &model, &prompt_tokens, actual_max, n_batch as usize, crate::config::llm_temperature(), None)
}

pub fn request_rag_llm(rules: &str, prompt: &str) -> String {
    request_rag_llm_inner(rules, prompt, crate::config::llm_temperature(), None)
}

pub fn request_rag_llm_with_temp(rules: &str, prompt: &str, temp: f32) -> String {
    request_rag_llm_inner(rules, prompt, temp, None)
}

pub fn request_rag_llm_streaming(
    rules: &str,
    prompt: &str,
    temp: f32,
    token_tx: tokio::sync::mpsc::Sender<String>,
) -> String {
    request_rag_llm_inner(rules, prompt, temp, Some(token_tx))
}

fn request_rag_llm_inner(rules: &str, prompt: &str, temp: f32, token_tx: Option<tokio::sync::mpsc::Sender<String>>) -> String {
    let model = {
        let g = crate::interface_llm::llama_cpp_4::MODEL_LOADED.lock().unwrap();
        match g.as_ref() {
            Some(arc) => std::sync::Arc::clone(arc),
            None => { tracing::error!("[REQUEST_RAG_LLM] Modèle non chargé."); return "Erreur: Modèle non chargé.".to_string(); }
        }
    };
    let backend = {
        let g = crate::interface_llm::llama_cpp_4::MODEL_BACKEND.lock().unwrap();
        match g.as_ref() {
            Some(arc) => std::sync::Arc::clone(arc),
            None => { tracing::error!("[REQUEST_RAG_LLM] Backend non initialisé."); return "Erreur: Backend non initialisé.".to_string(); }
        }
    };

    const MAX_CONTEXT_CHARS: usize = 3000;
    const DEFAULT_MAX_NEW_TOKENS: usize = 4096;
    const MIN_NEW_TOKENS: usize = 256;
    const CHUNK_LINES: usize = 3;

    let env_cap = crate::config::llm_max_new_tokens();
    let max_new_tokens_cap = if env_cap > 0 { env_cap } else { DEFAULT_MAX_NEW_TOKENS };

    let ctx_params_embed = get_ctx_params_config_embed();
    let (ctx_params_gen, n_ctx, n_batch) = get_ctx_params_config();

    let chunks: Vec<String> = chunk_rules(rules, CHUNK_LINES, MAX_CONTEXT_CHARS);
    tracing::info!("[REQUEST_RAG_LLM] {} chunks à embedder", chunks.len());

    let rag_context = {
        let mut embed_ctx = match model.new_context(&backend, ctx_params_embed) {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::error!("[REQUEST_RAG_LLM] Erreur contexte embed: {}", e);
                return rules.chars().take(MAX_CONTEXT_CHARS).collect();
            }
        };

        let query = "points essentiels résumé document";
        let query_emb = embed_text(&mut embed_ctx, &model, query);

        let mut scored: Vec<(f32, String)> = chunks
            .iter()
            .filter_map(|chunk| {
                if chunk.trim().is_empty() { return None; }
                let emb = embed_text(&mut embed_ctx, &model, chunk);
                Some((cosine_similarity(&query_emb, &emb), chunk.clone()))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut context = String::new();
        for (_, chunk) in &scored {
            if context.len() + chunk.len() + 2 > MAX_CONTEXT_CHARS { break; }
            context.push_str(chunk);
            context.push_str("\n\n");
        }

        if context.trim().is_empty() {
            tracing::warn!("[REQUEST_RAG_LLM] Rules inexploitable.");
            return "Rules inexploitable.".to_string();
        }

        tracing::info!("[REQUEST_RAG_LLM] Contexte RAG: {} chars", context.len());
        context
    };

    let prompt_str = format!(
        "<s><|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant",
        rag_context, prompt
    );

    let mut gen_ctx = match model.new_context(&backend, ctx_params_gen) {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!("[REQUEST_RAG_LLM] Erreur contexte génération: {}", e);
            return "Erreur: impossible de créer le contexte de génération.".to_string();
        }
    };

    let mut prompt_tokens = match model.str_to_token(&prompt_str, AddBos::Never) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("[REQUEST_RAG_LLM] Erreur tokenisation: {}", e);
            return "Erreur: tokenisation échouée.".to_string();
        }
    };

    let max_prompt_tokens = (n_ctx as usize).saturating_sub(MIN_NEW_TOKENS + 64).max(64);
    if prompt_tokens.len() > max_prompt_tokens {
        tracing::warn!("[REQUEST_RAG_LLM] Prompt trop long ({} tokens), troncature à {}", prompt_tokens.len(), max_prompt_tokens);
        prompt_tokens.truncate(max_prompt_tokens);
    }

    let prompt_len = prompt_tokens.len();
    let available  = (n_ctx as usize).saturating_sub(prompt_len + 64);
    let actual_max = max_new_tokens_cap.min(available).max(MIN_NEW_TOKENS) as i32;

    tracing::info!("[REQUEST_RAG_LLM] Prompt: {} tokens | Génération max: {} tokens | temp={}", prompt_len, actual_max, temp);

    run_generation(&mut gen_ctx, &model, &prompt_tokens, actual_max, n_batch as usize, temp, token_tx.as_ref())
}
