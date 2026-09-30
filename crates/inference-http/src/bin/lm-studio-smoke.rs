use anyhow::{Context, Result};
use pet_inference_http::{ChatMessage, LmStudioBackend, LmStudioConfig};

#[tokio::main]
async fn main() -> Result<()> {
    let endpoint = std::env::args()
        .nth(1)
        .context("usage: lm-studio-smoke URL MODEL_ID [PROMPT]")?;
    let model = std::env::args()
        .nth(2)
        .context("usage: lm-studio-smoke URL MODEL_ID [PROMPT]")?;
    let prompt = std::env::args().nth(3);
    let mut config = LmStudioConfig::new(endpoint, model);
    config.disable_thinking = std::env::args().any(|arg| arg == "--no-thinking");
    let backend = LmStudioBackend::new(config)?;
    backend.probe().await?;
    println!("Model listed: {}", backend.model_id());
    if let Some(prompt) = prompt {
        let answer = if std::env::args().nth(4).as_deref() == Some("--compat") {
            backend
                .stream_chat(
                    &[
                        ChatMessage::system("请用简短中文回答，只输出适合朗读的正文。"),
                        ChatMessage::user(prompt),
                    ],
                    |delta| print!("{delta}"),
                )
                .await?
        } else {
            backend
                .stream_native_chat(
                    &prompt,
                    "请用简短中文回答，只输出适合朗读的正文。",
                    |delta| print!("{delta}"),
                )
                .await?
        };
        println!("\nReply characters: {}", answer.chars().count());
    }
    Ok(())
}
