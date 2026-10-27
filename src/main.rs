// Release builds are a GUI app: no console window on Windows (logs go to the log file).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tracked::config::{self, RendererPref, Settings};
use tracked::{app::App, ipc, logging, worker};

const MIN_SIZE: [f32; 2] = [1000.0, 660.0];

fn options(settings: &Settings, renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tracked")
            .with_inner_size([
                settings.window_size[0].max(MIN_SIZE[0]),
                settings.window_size[1].max(MIN_SIZE[1]),
            ])
            .with_maximized(settings.window_maximized)
            .with_min_inner_size(MIN_SIZE)
            // The app draws its own slim title bar unless the user opted out.
            .with_decorations(settings.system_title_bar),
        renderer,
        vsync: true,
        ..Default::default()
    }
}

fn run(settings: Settings, demo: bool, renderer: eframe::Renderer) -> eframe::Result<()> {
    eframe::run_native(
        "Tracked",
        options(&settings, renderer),
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            tracked::ui::theme::install_fonts(&cc.egui_ctx);
            let gpu = tracked::ui::theme::describe_renderer(cc);
            if !settings.system_title_bar {
                tracked::ui::titlebar::native_frame(cc);
            }
            tracing::info!(renderer = %gpu, "renderer ready");
            let (intent_tx, intent_rx, sink, update_rx) = ipc::channels(cc.egui_ctx.clone());
            let handle = worker::spawn_mode(intent_rx, sink, settings.clone(), demo);
            Ok(Box::new(App::new(settings, intent_tx, update_rx, handle, gpu)))
        }),
    )
}

fn main() -> eframe::Result<()> {
    let debug = std::env::args().any(|a| a == "--debug");
    let demo = std::env::args().any(|a| a == "--demo");
    let force_gl = std::env::args().any(|a| a == "--opengl");
    let _log_guard = logging::init(debug);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting tracked");

    let settings = config::load();
    if force_gl || settings.renderer == RendererPref::OpenGl {
        return run(settings, demo, eframe::Renderer::Glow);
    }
    // GPU via wgpu (Direct3D 12 / Vulkan). winit can't create a second event
    // loop in one process, so the OpenGL fallback relaunches the app.
    match run(settings, demo, eframe::Renderer::Wgpu) {
        Err(e) => {
            tracing::warn!("wgpu renderer failed ({e}); relaunching with OpenGL");
            relaunch_with_opengl().map_err(|_| e)
        }
        ok => ok,
    }
}

/// Run this executable again with `--opengl` and wait for it.
fn relaunch_with_opengl() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .arg("--opengl")
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}
