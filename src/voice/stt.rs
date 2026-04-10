use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Stt {
    ctx: WhisperContext,
    language: String,
}

impl Stt {
    pub fn load(model_path: &str, language: &str) -> Result<Self> {
        if !std::path::Path::new(model_path).exists() {
            anyhow::bail!(
                "whisper model not found at {model_path} — download a ggml model \
                 (e.g. ggml-base.en.bin from huggingface.co/ggerganov/whisper.cpp) \
                 and set [voice] model in config.toml"
            );
        }
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .context("loading whisper model")?;
        let stt = Self {
            ctx,
            language: language.to_string(),
        };
        // Warmup forces GGML to allocate per-state buffers so the first real
        // utterance doesn't pay that cost on top of normal transcribe time.
        let silence = vec![0.0_f32; 16000 / 2];
        let _ = stt.transcribe(&silence, "");
        Ok(stt)
    }

    pub fn transcribe(&self, pcm: &[f32], initial_prompt: &str) -> Result<String> {
        let mut state = self
            .ctx
            .create_state()
            .context("creating whisper state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_language(Some(self.language.as_str()));
        params.set_translate(false);
        params.set_n_threads(num_threads());
        if !initial_prompt.is_empty() {
            params.set_initial_prompt(initial_prompt);
        }
        state.full(params, pcm).context("whisper full() failed")?;

        let segments = state.full_n_segments();
        let mut out = String::new();
        for i in 0..segments {
            let Some(seg) = state.get_segment(i) else {
                continue;
            };
            let text = seg.to_str().context("whisper segment text decode")?;
            out.push_str(text);
        }
        Ok(out.trim().to_string())
    }
}

fn num_threads() -> std::os::raw::c_int {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    n.clamp(1, 8) as std::os::raw::c_int
}
