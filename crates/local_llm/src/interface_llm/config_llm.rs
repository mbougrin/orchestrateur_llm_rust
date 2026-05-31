use std::num::NonZeroU32;
use llama_cpp_4::context::params::{LlamaContextParams, LlamaPoolingType};
use llama_cpp_4::model::params::LlamaModelParams;
use llama_cpp_4::quantize::GgmlType;
use llama_cpp_4::sampling::LlamaSampler;
use crate::config::{llm_n_ctx, llm_n_gpu_layers, llm_n_threads, llm_seed, llm_temperature, llm_top_k, llm_top_p};
use crate::hardware::hardware_profile::HardwareProfile;

pub fn get_sample_config_resume() -> LlamaSampler {
    get_sample_config_with_temp(llm_temperature())
}

pub fn get_sample_config_with_temp(temp: f32) -> LlamaSampler {
    LlamaSampler::chain_simple([
        LlamaSampler::top_k(llm_top_k()),
        LlamaSampler::top_p(llm_top_p(), 1),
        LlamaSampler::temp(temp),
        LlamaSampler::dist(llm_seed()),
    ])
}

pub fn get_config_model_params() -> LlamaModelParams {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let gpu_layers = llm_n_gpu_layers().unwrap_or(0);
        let use_mlock = true;
        tracing::info!("[CONFIG] OS: Win/Linux | Mode: CPU | Layers: {} | mlock: {}", gpu_layers, use_mlock);

        LlamaModelParams::default()
            .with_n_gpu_layers(gpu_layers)
            .with_use_mlock(use_mlock)
            .with_main_gpu(0)
            .with_vocab_only(false)
    }

    #[cfg(target_os = "macos")]
    {
        let gpu_layers = llm_n_gpu_layers().unwrap_or(999);
        let use_mlock = false;
        tracing::info!("[CONFIG] OS: MacOS | Mode: Metal | Layers: {}", gpu_layers);

        LlamaModelParams::default()
            .with_n_gpu_layers(gpu_layers)
            .with_use_mlock(use_mlock)
            .with_main_gpu(0)
            .with_vocab_only(false)
    }
}

pub fn get_ctx_params_config_embed() -> LlamaContextParams {
    let hw = HardwareProfile::detect();
    let _ram = hw.total_ram_gb as i32;

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let threads = (hw.physical_cores as i32).max(2).min(8);
        let n_batch = 128;
        let ctx = if _ram <= 8 { 1024 } else { 2048 };
        let (cache_k, cache_v) = (GgmlType::Q8_0, GgmlType::Q8_0);

        tracing::info!("[CONFIG] Embed CPU | Ctx: {} | Threads: {} | Cache: Q8_0", ctx, threads);

        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(64)
            .with_n_threads(threads)
            .with_n_threads_batch(threads)
            .with_embeddings(true)
            .with_cache_type_k(cache_k)
            .with_cache_type_v(cache_v)
            .with_pooling_type(LlamaPoolingType::Mean)
            .with_offload_kqv(false)
    }

    #[cfg(target_os = "macos")]
    {
        let ctx = 4096;
        let n_batch = 1024;

        tracing::info!("[CONFIG] Embed MacOS | Ctx: {} | Mode: F16 GPU", ctx);

        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(n_batch)
            .with_n_threads(hw.physical_cores as i32)
            .with_n_threads_batch(hw.logical_cores as i32)
            .with_embeddings(true)
            .with_cache_type_k(GgmlType::F16)
            .with_cache_type_v(GgmlType::F16)
            .with_pooling_type(LlamaPoolingType::Mean)
            .with_offload_kqv(true)
    }
}

pub fn get_ctx_params_config() -> (LlamaContextParams, u32, u32) {
    let hw = HardwareProfile::detect();
    let ram = hw.total_ram_gb as i32;

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let threads = llm_n_threads().unwrap_or((hw.physical_cores as i32).max(2).min(8));

        let (default_ctx, n_batch, cache_k, cache_v) = match ram {
            r if r <= 8  => (2048u32, 128u32, GgmlType::Q4_0, GgmlType::Q4_0),
            r if r <= 16 => (4096,    256,    GgmlType::Q4_0, GgmlType::Q4_0),
            _            => (8192,    512,    GgmlType::Q5_0, GgmlType::Q5_0),
        };
        let ctx = llm_n_ctx().unwrap_or(default_ctx);

        tracing::info!("[CONFIG] Gen CPU | Ctx: {} | Batch: {} | Threads: {}", ctx, n_batch, threads);

        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(n_batch / 2)
            .with_n_threads(threads)
            .with_n_threads_batch(threads)
            .with_embeddings(false)
            .with_cache_type_k(cache_k)
            .with_cache_type_v(cache_v)
            .with_offload_kqv(false);

        (params, ctx, n_batch)
    }

    #[cfg(target_os = "macos")]
    {
        let (default_ctx, n_batch) = match ram {
            r if r <= 8  => (8192u32,  512u32),
            r if r <= 16 => (16384, 1024),
            _            => (32768, 2048),
        };
        let ctx = llm_n_ctx().unwrap_or(default_ctx);
        let threads     = llm_n_threads().unwrap_or(hw.physical_cores as i32);
        let threads_bat = llm_n_threads().unwrap_or(hw.logical_cores  as i32);

        tracing::info!("[CONFIG] Gen MacOS | Ctx: {} | Batch: {} | Threads: {}", ctx, n_batch, threads);

        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(n_batch)
            .with_n_threads(threads)
            .with_n_threads_batch(threads_bat)
            .with_embeddings(false)
            .with_cache_type_k(GgmlType::F16)
            .with_cache_type_v(GgmlType::F16)
            .with_offload_kqv(true);

        (params, ctx, n_batch)
    }
}
