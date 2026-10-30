#[tokio::main]
async fn main() -> anyhow::Result<()> {
    disco_snails::cli::run(true).await
}
