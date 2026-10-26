use ascend_lite::{app::App, config, ipc, logging, worker};

fn main() -> eframe::Result<()> {
    let debug = std::env::args().any(|a| a == "--debug");
    let demo = std::env::args().any(|a| a == "--demo");
    let _log_guard = logging::init(debug);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting ascend-lite");

    let settings = config::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ascend-Lite")
            .with_inner_size(settings.window_size)
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ascend-Lite",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let (intent_tx, intent_rx, sink, update_rx) = ipc::channels(cc.egui_ctx.clone());
            let handle = worker::spawn_mode(intent_rx, sink, settings.clone(), demo);
            Ok(Box::new(App::new(settings, intent_tx, update_rx, handle)))
        }),
    )
}
