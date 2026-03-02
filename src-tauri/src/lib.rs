#[cfg(target_os = "macos")]
mod apps;
#[cfg(target_os = "macos")]
mod window_manager;

#[cfg(target_os = "macos")]
use apps::get_running_gui_apps;
#[cfg(target_os = "macos")]
use window_manager::{get_window_id_for_pid, get_window_level, pin_window, unpin_window};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder},
    AppHandle, Manager, Wry,
};

pub struct PinnedEntry {
    pub name: String,
    pub pid: i32,
    pub window_id: u32,
    pub original_level: i32,
}

pub type PinnedApps = Arc<Mutex<HashMap<i32, PinnedEntry>>>;

// ID del tray icon (generato al runtime, salvato per poter aggiornare il menu)
pub type TrayIdState = Arc<Mutex<Option<String>>>;

fn build_tray_menu(app: &AppHandle, pinned: &PinnedApps) -> Result<Menu<Wry>, tauri::Error> {
    let guard = pinned.lock().unwrap();

    #[cfg(target_os = "macos")]
    let running_apps = get_running_gui_apps();
    #[cfg(not(target_os = "macos"))]
    let running_apps: Vec<crate::apps::RunningApp> = vec![];

    let menu = Menu::new(app)?;

    for running_app in &running_apps {
        let label = if guard.contains_key(&running_app.pid) {
            format!("✓ {}", running_app.name)
        } else {
            running_app.name.clone()
        };
        let id = format!("pin_{}", running_app.pid);
        let item = MenuItem::with_id(app, id, label, true, None::<&str>)?;
        menu.append(&item)?;
    }

    if !running_apps.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }

    menu.append(&MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?)?;

    Ok(menu)
}

/// Ricostruisce il menu del tray e lo aggiorna (per riflettere pin/unpin e nuove app).
fn refresh_tray_menu(app: &AppHandle) {
    let tray_id = app
        .state::<TrayIdState>()
        .lock()
        .unwrap()
        .clone();

    if let Some(id) = tray_id {
        if let Some(tray) = app.tray_by_id(&id) {
            let pinned = app.state::<PinnedApps>().inner().clone();
            if let Ok(menu) = build_tray_menu(app, &pinned) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }
}

pub fn run() {
    let pinned_apps: PinnedApps = Arc::new(Mutex::new(HashMap::new()));
    let tray_id_state: TrayIdState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .manage(pinned_apps)
        .manage(tray_id_state)
        .setup(|app| {
            // Menu bar only — nascondi dal Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let icon = Image::new(include_bytes!("../icons/tray.rgba"), 32, 32);

            let pinned = app.state::<PinnedApps>().inner().clone();
            let initial_menu = build_tray_menu(app.handle(), &pinned)?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .menu(&initial_menu)
                .show_menu_on_left_click(true) // ← click sinistro
                .on_menu_event(|app: &AppHandle, event| {
                    let id = event.id.as_ref();

                    if id == "quit" {
                        let pinned = app.state::<PinnedApps>().inner().clone();
                        let mut guard = pinned.lock().unwrap();
                        for entry in guard.values() {
                            #[cfg(target_os = "macos")]
                            let _ = unpin_window(entry.window_id, entry.original_level);
                        }
                        guard.clear();
                        drop(guard);
                        app.exit(0);
                    } else if let Some(pid_str) = id.strip_prefix("pin_") {
                        if let Ok(pid) = pid_str.parse::<i32>() {
                            handle_pin_toggle(app, pid);
                            // Aggiorna il menu per mostrare/rimuovere il checkmark
                            refresh_tray_menu(app);
                        }
                    }
                })
                .build(app)?;

            // Salva l'ID del tray per poterlo recuperare in seguito
            let id_str = tray.id().0.clone();
            *app.state::<TrayIdState>().lock().unwrap() = Some(id_str);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("errore durante l'avvio di Merlino");
}

fn handle_pin_toggle(app: &AppHandle, pid: i32) {
    let pinned = app.state::<PinnedApps>().inner().clone();
    let mut guard = pinned.lock().unwrap();

    if let Some(entry) = guard.remove(&pid) {
        // DE-PIN: ripristina il livello originale
        #[cfg(target_os = "macos")]
        if let Err(e) = unpin_window(entry.window_id, entry.original_level) {
            eprintln!("unpin_window error: {}", e);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = entry;
    } else {
        // PIN: ottieni window ID, salva livello originale, imposta floating
        #[cfg(target_os = "macos")]
        {
            let Some(wid) = get_window_id_for_pid(pid) else {
                eprintln!("Nessuna finestra trovata per PID {}", pid);
                return;
            };
            let original_level = get_window_level(wid);
            match pin_window(wid) {
                Ok(()) => {
                    let name = get_running_gui_apps()
                        .into_iter()
                        .find(|a| a.pid == pid)
                        .map(|a| a.name)
                        .unwrap_or_else(|| format!("PID {}", pid));
                    guard.insert(
                        pid,
                        PinnedEntry {
                            name,
                            pid,
                            window_id: wid,
                            original_level,
                        },
                    );
                }
                Err(e) => eprintln!("pin_window error: {}", e),
            }
        }
    }
}
