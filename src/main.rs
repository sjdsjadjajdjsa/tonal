mod app;
mod artwork;
mod integrations;
mod library;
mod media_controls;
mod model;
mod player;
mod sources;
mod visualizer;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    app::run()
}
