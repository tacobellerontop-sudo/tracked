use ascend_lite::{app::App, ipc, worker};

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ascend-Lite")
            .with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ascend-Lite",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let (intent_tx, intent_rx, sink, update_rx) = ipc::channels(cc.egui_ctx.clone());
            let handle = worker::spawn(intent_rx, sink);
            intent_tx.send(ipc::Intent::Ping(0));
            Ok(Box::new(App::new(intent_tx, update_rx, handle)))
        }),
    )
}
