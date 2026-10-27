// Release builds are a GUI app: no console window on Windows (logs go to the log file).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tracked::config::{self, Settings};
use tracked::{app::App, ipc, logging, worker};

const MIN_SIZE: [f32; 2] = [1000.0, 660.0];

fn options(settings: &Settings, renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        // If wgpu is chosen, keep at most one frame queued: its default
        // queue added visible input lag on some Windows drivers.
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            desired_maximum_frame_latency: Some(1),
            ..Default::default()
        },
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
    let force_wgpu = std::env::args().any(|a| a == "--wgpu");
    let _log_guard = logging::init(debug);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting tracked");

    let settings = config::load();
    // OpenGL (on the GPU) by default; wgpu (Direct3D 12 / Vulkan) when chosen.
    // A forced flag means we're the fallback relaunch: no further fallback.
    let (first, fallback) = if force_gl {
        (eframe::Renderer::Glow, None)
    } else if force_wgpu {
        (eframe::Renderer::Wgpu, None)
    } else if settings.renderer.uses_wgpu() {
        (eframe::Renderer::Wgpu, Some("--opengl"))
    } else {
        (eframe::Renderer::Glow, Some("--wgpu"))
    };
    // winit can't create a second event loop in one process, so the fallback
    // relaunches the app with the other renderer.
    match (run(settings, demo, first), fallback) {
        (Err(e), Some(flag)) => {
            tracing::warn!("{first:?} renderer failed ({e}); relaunching with {flag}");
            relaunch_with(flag).map_err(|_| e)
        }
        (result, _) => result,
    }
}

/// Run this executable again with `flag` and wait for it.
fn relaunch_with(flag: &str) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .arg(flag)
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}
