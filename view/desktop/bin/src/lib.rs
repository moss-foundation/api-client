pub mod constants;
mod commands;
mod mem;
mod menu;
mod plugins;
mod utl;
mod window;

#[macro_use]
extern crate tracing;

use anyhow::Result;

use tauri::{AppHandle, Manager, RunEvent, WebviewWindow, WindowEvent};

use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_os;

use rand::random;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{Layer};
use tracing_subscriber::layer::SubscriberExt;

use moss_desktop::app::manager::AppManager;
use moss_desktop::app::state::AppStateManager;

use crate::commands::*;
use crate::plugins::*;

pub use constants::*;
use moss_desktop::services::window_service::WindowService;
use window::{create_window, CreateWindowInput};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(plugin_log::init())
        .plugin(plugin_window_state::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init());

    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(mac_window::init());
    }

    builder
        .setup(|app| {
            let log_format = tracing_subscriber::fmt::format()
                .with_file(true)
                .with_line_number(true)
                .with_target(false)
                .compact();

            let log_level_filter = std::env::var("LOG_LEVEL")
                .unwrap_or("trace".to_string())
                .to_lowercase()
                .parse()
                .unwrap_or(LevelFilter::TRACE);

            let subscriber = tracing_subscriber::registry().with(
                tracing_subscriber::fmt::layer()
                    .event_format(log_format)
                    .with_ansi(true)
                    .with_span_events(FmtSpan::CLOSE)
                    .with_filter(log_level_filter),
            );

            tracing::subscriber::set_global_default(subscriber)
                .expect("failed to set tracing subscriber");

            let app_handle = app.app_handle();

            let app_state = AppStateManager::new();
            app_handle.manage(app_state);

            let app_manager = AppManager::new(app_handle.clone());
            // TODO: Service creation and registry?
            app_handle.manage(app_manager);

            let ctrl_n_shortcut = Shortcut::new(Some(Modifiers::CONTROL), Code::KeyN);

            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, shortcut, event| {
                        println!("{:?}", shortcut);
                        if shortcut == &ctrl_n_shortcut {
                            match event.state() {
                                ShortcutState::Pressed => {
                                    tauri::async_runtime::spawn(cmd_window::create_new_window(
                                        app.clone(),
                                    ));
                                }
                                ShortcutState::Released => {}
                            }
                        }
                    })
                    .build(),
            )?;
            app.global_shortcut().register(ctrl_n_shortcut)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cmd_window::create_new_window,
            cmd_window::get_state,
        ])
        .on_window_event(|window, event| match event {
            #[cfg(target_os = "macos")]
            WindowEvent::CloseRequested { api, .. } => {
                if window.app_handle().webview_windows().len() == 1 {
                    window.app_handle().hide().ok();
                    api.prevent_close();
                }
            }
            WindowEvent::Focused(_) => { /* call updates, git fetch, etc. */ }

            _ => (),
        })
        .build(tauri::generate_context!())
        .expect("failed to run")
        .run(|app_handle, event| match event {
            RunEvent::Ready => {
                let webview_window = create_main_window(&app_handle, "/");
                webview_window
                    .on_menu_event(move |window, event| menu::handle_event(window, &event));
            }

            #[cfg(target_os = "macos")]
            RunEvent::ExitRequested { api, .. } => {
                app_handle.hide().ok();
                api.prevent_exit();
            }

            _ => {}
        });
}

fn create_main_window(app_handle: &AppHandle, url: &str) -> WebviewWindow {
    // TODO: Use ConfigurationService

    let window_inner_height = DEFAULT_WINDOW_HEIGHT;

    let window_inner_width = DEFAULT_WINDOW_WIDTH;

    dbg!(&window_inner_width, &window_inner_height);

    let label = format!("{MAIN_WINDOW_PREFIX}{}", 0);
    let config = CreateWindowInput {
        url,
        label: label.as_str(),
        title: "Moss Studio",
        inner_size: (window_inner_width, window_inner_height),
        position: (
            100.0,
            100.0,
        ),
    };

    create_window(app_handle, config)
}

fn create_child_window(app_handle: &AppHandle, url: &str) -> Result<WebviewWindow> {
    let app_manager = app_handle.state::<AppManager>();
    let next_window_id = app_manager.service::<WindowService>()?.next_window_id() + 1;
    let config = CreateWindowInput {
        url,
        label: &format!("{MAIN_WINDOW_PREFIX}{}", next_window_id),
        title: "Moss Studio",
        inner_size: (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT),
        position: (
            100.0 + random::<f64>() * 20.0,
            100.0 + random::<f64>() * 20.0,
        ),
    };

    Ok(create_window(app_handle, config))
}
