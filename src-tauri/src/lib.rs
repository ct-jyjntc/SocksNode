mod manager;
mod models;

use manager::{open_path, resolve_paths, Manager};
use models::{AppInfo, LogEntry, NodeInput, NodeTestResult, ProxyNode};
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager as TauriManager, RunEvent, WindowEvent,
};

struct AppState {
    manager: Arc<Manager>,
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = app.show();
    }
}

fn hide_to_tray(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

fn rebuild_tray_tooltip(app: &AppHandle, manager: &Manager) {
    let (running, total) = manager.status_summary();
    let tip = if total == 0 {
        "SocksNode".to_string()
    } else {
        format!("SocksNode · {running}/{total} 运行中")
    };
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_tooltip(Some(tip));
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_i = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let start_all_i = MenuItem::with_id(app, "start_all", "全部启动", true, None::<&str>)?;
    let stop_all_i = MenuItem::with_id(app, "stop_all", "全部停止", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &start_all_i, &stop_all_i, &quit_i])?;

    let icon = {
        let candidates = [
            app.path()
                .resource_dir()
                .ok()
                .map(|p| p.join("icons").join("tray.png")),
            Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/tray.png")),
            Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png")),
        ];
        let mut img = None;
        for c in candidates.into_iter().flatten() {
            if c.exists() {
                if let Ok(i) = Image::from_path(&c) {
                    img = Some(i);
                    break;
                }
            }
        }
        img.unwrap_or_else(|| {
            app.default_window_icon()
                .cloned()
                .expect("missing app icon")
        })
    };

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("SocksNode")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "start_all" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.manager.start_all();
                    rebuild_tray_tooltip(app, &state.manager);
                }
            }
            "stop_all" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.manager.stop_all();
                    rebuild_tray_tooltip(app, &state.manager);
                }
            }
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.manager.shutdown();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[tauri::command]
fn nodes_list(state: tauri::State<'_, AppState>) -> Vec<ProxyNode> {
    state.manager.list()
}

#[tauri::command]
fn nodes_create(
    state: tauri::State<'_, AppState>,
    input: NodeInput,
) -> Result<Vec<ProxyNode>, String> {
    state.manager.create(input)
}

#[tauri::command]
fn nodes_update(
    state: tauri::State<'_, AppState>,
    id: String,
    patch: NodeInput,
) -> Result<Vec<ProxyNode>, String> {
    state.manager.update(&id, patch)
}

#[tauri::command]
fn nodes_remove(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<ProxyNode>, String> {
    state.manager.remove(&id)
}

#[tauri::command]
fn nodes_start(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<ProxyNode>, String> {
    let result = state.manager.start(&id, false);
    rebuild_tray_tooltip(&app, &state.manager);
    result
}

#[tauri::command]
fn nodes_stop(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<ProxyNode>, String> {
    let result = state.manager.stop(&id, false);
    rebuild_tray_tooltip(&app, &state.manager);
    result
}

#[tauri::command]
fn nodes_start_all(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProxyNode>, String> {
    let result = state.manager.start_all();
    rebuild_tray_tooltip(&app, &state.manager);
    result
}

#[tauri::command]
fn nodes_stop_all(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProxyNode>, String> {
    let result = state.manager.stop_all();
    rebuild_tray_tooltip(&app, &state.manager);
    result
}

#[tauri::command]
fn nodes_test(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<NodeTestResult, String> {
    let result = state.manager.test(&id);
    rebuild_tray_tooltip(&app, &state.manager);
    result
}

#[tauri::command]
fn nodes_logs(state: tauri::State<'_, AppState>, id: String) -> Vec<LogEntry> {
    state.manager.get_logs(&id)
}

#[tauri::command]
fn app_get_info(state: tauri::State<'_, AppState>) -> AppInfo {
    state.manager.get_info(env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
fn app_open_path(path: String) -> Result<(), String> {
    open_path(&path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let (data_dir, resources_dir) = resolve_paths(app.handle());
            let manager = Arc::new(Manager::new(data_dir, resources_dir));
            manager.init().map_err(|e| e.to_string())?;
            manager.set_app(app.handle().clone());
            app.manage(AppState {
                manager: Arc::clone(&manager),
            });

            setup_tray(app.handle())?;
            rebuild_tray_tooltip(app.handle(), &manager);

            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_to_tray(&app_handle);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            nodes_list,
            nodes_create,
            nodes_update,
            nodes_remove,
            nodes_start,
            nodes_stop,
            nodes_start_all,
            nodes_stop_all,
            nodes_test,
            nodes_logs,
            app_get_info,
            app_open_path,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                RunEvent::Exit | RunEvent::ExitRequested { .. } => {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        state.manager.shutdown();
                    }
                }
                #[cfg(target_os = "macos")]
                RunEvent::Reopen {
                    has_visible_windows,
                    ..
                } => {
                    if !has_visible_windows {
                        show_main_window(app_handle);
                    }
                }
                _ => {}
            }
        });
}
