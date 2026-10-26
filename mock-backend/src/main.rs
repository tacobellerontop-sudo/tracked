//! `cargo run -p mock-backend` — serves the mock on 127.0.0.1:8787
//! (override with `MOCK_ADDR`). `MOCK_MIN_VERSION` sets the version gate.

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let addr = std::env::var("MOCK_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    tracing::info!("mock backend listening on http://{addr}");
    mock_backend::serve(listener).await;
}
