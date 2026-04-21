//! Live smoke test for Reflexion against a real OpenAI-compatible endpoint.
//!
//! Usage (no keys in source — set via env):
//!
//!     export INKWORM_LLM_BASE_URL="https://api.openai.com/v1"
//!     export INKWORM_LLM_API_KEY="sk-..."
//!     export INKWORM_LLM_MODEL="gpt-4o-mini"
//!     # Write an article to a file, point the example at it:
//!     echo "This is the article body..." > /tmp/article.txt
//!     cargo run --example smoke -- /tmp/article.txt
//!
//! Prints the resulting Course JSON on success, or a diagnostic on failure.

use std::path::PathBuf;
use std::time::Duration;

use inkworm::clock::SystemClock;
use inkworm::llm::client::ReqwestClient;
use inkworm::llm::reflexion::Reflexion;
use inkworm::storage::paths::DataPaths;
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let article_path: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: smoke <article-path>"))?
        .into();
    let article = std::fs::read_to_string(&article_path)?;

    let base_url = std::env::var("INKWORM_LLM_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let api_key = std::env::var("INKWORM_LLM_API_KEY")
        .map_err(|_| anyhow::anyhow!("INKWORM_LLM_API_KEY not set"))?;
    let model = std::env::var("INKWORM_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());

    // Use a temp data dir so the smoke run doesn't pollute ~/.config/inkworm.
    let tmp = tempfile::tempdir()?;
    let paths = DataPaths::resolve(Some(tmp.path()))?;
    paths.ensure_dirs()?;

    let clock = SystemClock;
    let client = ReqwestClient::new(base_url, api_key, Duration::from_secs(60))?;
    let r = Reflexion {
        client: &client,
        clock: &clock,
        paths: &paths,
        model: &model,
        max_concurrent: 5,
        cancel: CancellationToken::new(),
    };

    eprintln!("Generating course from {article_path:?} …");
    let t0 = std::time::Instant::now();
    let outcome = r.generate(&article, &[], None).await?;
    let elapsed = t0.elapsed();
    eprintln!("Done in {elapsed:.2?}. Course:");
    println!("{}", serde_json::to_string_pretty(&outcome.course)?);
    Ok(())
}
