//! Authenticated access to the autonomous small syntax worker.
use crate::{llm, models::GrammarAnalysis, runtime::Runtime};
use std::time::Duration;

/// Parses English locally; neither the sentence nor a screenshot is sent to the network.
pub async fn analyze(rt: &Runtime, context: &str) -> Result<GrammarAnalysis, String> {
    if context.len() > 12000 {
        return Err("Слишком длинная реплика для разбора".into());
    }
    let config = rt.inner.lock().unwrap().data.settings.translator.clone();
    let mini = rt.mini.clone();
    let threads = llm::threads(&config);
    let (endpoint, token) =
        tauri::async_runtime::spawn_blocking(move || mini.start_grammar(threads))
            .await
            .map_err(|e| e.to_string())??;
    rt.wait_model(&endpoint, &token).await?;
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("{endpoint}/analyze"))
        .bearer_auth(token)
        .json(&serde_json::json!({"context":context}))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<GrammarAnalysis>()
        .await
        .map_err(|e| e.to_string())
}
