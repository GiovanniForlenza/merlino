use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebApp {
    pub id: String,
    pub name: String,
    pub url: String,
}

pub type WebApps = Arc<Mutex<Vec<WebApp>>>;
pub type TrayIdState = Arc<Mutex<Option<String>>>;

// ── Persistenza ───────────────────────────────────────────────────────────────

fn webapps_path(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("webapps.json")
}

fn load_webapps(app: &AppHandle) -> Vec<WebApp> {
    let path = webapps_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_webapps(app: &AppHandle, webapps: &[WebApp]) {
    let path = webapps_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(webapps) {
        let _ = std::fs::write(path, json);
    }
}

// ── Menu ──────────────────────────────────────────────────────────────────────

fn is_window_open(app: &AppHandle, id: &str) -> bool {
    app.get_webview_window(&format!("webapp_{}", id)).is_some()
}

fn build_tray_menu(app: &AppHandle, webapps: &WebApps) -> Result<Menu<Wry>, tauri::Error> {
    let guard = webapps.lock().unwrap();
    let menu = Menu::new(app)?;

    for wa in guard.iter() {
        let open = is_window_open(app, &wa.id);
        let label = if open {
            format!("✓ {}", wa.name)
        } else {
            wa.name.clone()
        };
        let item = MenuItem::with_id(app, format!("webapp_{}", wa.id), label, true, None::<&str>)?;
        menu.append(&item)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "add_webapp", "Aggiungi web app…", true, None::<&str>)?)?;

    if !guard.is_empty() {
        let remove_sub = Submenu::new(app, "Rimuovi", true)?;
        for wa in guard.iter() {
            let item = MenuItem::with_id(
                app,
                format!("remove_{}", wa.id),
                &wa.name,
                true,
                None::<&str>,
            )?;
            remove_sub.append(&item)?;
        }
        menu.append(&remove_sub)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?)?;

    Ok(menu)
}

fn refresh_tray_menu(app: &AppHandle) {
    let tray_id = app.state::<TrayIdState>().lock().unwrap().clone();
    if let Some(id) = tray_id {
        if let Some(tray) = app.tray_by_id(&id) {
            let webapps = app.state::<WebApps>().inner().clone();
            if let Ok(menu) = build_tray_menu(app, &webapps) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }
}

// ── Finestre web app ──────────────────────────────────────────────────────────

fn open_webapp_window(app: &AppHandle, wa: &WebApp) {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let label = format!("webapp_{}", wa.id);

    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.set_focus();
        return;
    }

    let url = match wa.url.parse::<tauri::Url>() {
        Ok(u) => WebviewUrl::External(u),
        Err(e) => {
            eprintln!("[merlino] URL non valido '{}': {}", wa.url, e);
            return;
        }
    };

    match tauri::WebviewWindowBuilder::new(app, &label, url)
        .title(&wa.name)
        .always_on_top(true)
        .decorations(true)
        .resizable(true)
        .inner_size(1200.0, 800.0)
        .build()
    {
        Ok(_) => eprintln!("[merlino] Aperta web app '{}' → {}", wa.name, wa.url),
        Err(e) => eprintln!("[merlino] Errore apertura '{}': {}", wa.name, e),
    }
}

fn open_add_window(app: &AppHandle) {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    if let Some(win) = app.get_webview_window("add_webapp") {
        let _ = win.set_focus();
        return;
    }

    let _ = WebviewWindowBuilder::new(app, "add_webapp", WebviewUrl::App("add.html".into()))
        .title("Aggiungi web app")
        .always_on_top(true)
        .decorations(true)
        .resizable(false)
        .inner_size(420.0, 220.0)
        .build();
}

// ── Comandi Tauri ─────────────────────────────────────────────────────────────

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect()
}

#[tauri::command]
fn add_webapp(app: AppHandle, name: String, url: String) {
    let id = slugify(&name);
    let wa = WebApp { id: id.clone(), name, url };

    let webapps = app.state::<WebApps>().inner().clone();
    {
        let mut guard = webapps.lock().unwrap();
        if !guard.iter().any(|w| w.id == id) {
            guard.push(wa.clone());
        }
        save_webapps(&app, &guard);
    }

    if let Some(win) = app.get_webview_window("add_webapp") {
        let _ = win.close();
    }

    refresh_tray_menu(&app);
    open_webapp_window(&app, &wa);
}

#[tauri::command]
fn remove_webapp(app: AppHandle, id: String) {
    let webapps = app.state::<WebApps>().inner().clone();
    {
        let mut guard = webapps.lock().unwrap();
        guard.retain(|w| w.id != id);
        save_webapps(&app, &guard);
    }

    if let Some(win) = app.get_webview_window(&format!("webapp_{}", id)) {
        let _ = win.close();
    }

    refresh_tray_menu(&app);
}

// ── Setup app ─────────────────────────────────────────────────────────────────

pub fn run() {
    let webapps: WebApps = Arc::new(Mutex::new(vec![]));
    let tray_id_state: TrayIdState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .manage(webapps)
        .manage(tray_id_state)
        .invoke_handler(tauri::generate_handler![add_webapp, remove_webapp])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Carica web apps salvate
            let loaded = load_webapps(app.handle());
            eprintln!("[merlino] Caricate {} web app", loaded.len());
            *app.state::<WebApps>().lock().unwrap() = loaded;

            let icon = Image::new(include_bytes!("../icons/tray.rgba"), 32, 32);

            let webapps = app.state::<WebApps>().inner().clone();
            let initial_menu = build_tray_menu(app.handle(), &webapps)?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .menu(&initial_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app: &AppHandle, event| {
                    let id = event.id.as_ref();

                    if id == "quit" {
                        std::process::exit(0);
                    } else if id == "add_webapp" {
                        open_add_window(app);
                    } else if let Some(webapp_id) = id.strip_prefix("remove_") {
                        let webapps = app.state::<WebApps>().inner().clone();
                        {
                            let mut guard = webapps.lock().unwrap();
                            guard.retain(|w| w.id != webapp_id);
                            save_webapps(app, &guard);
                        }
                        if let Some(win) = app.get_webview_window(&format!("webapp_{}", webapp_id)) {
                            let _ = win.close();
                        }
                        refresh_tray_menu(app);
                    } else if let Some(webapp_id) = id.strip_prefix("webapp_") {
                        let webapps = app.state::<WebApps>().inner().clone();
                        let guard = webapps.lock().unwrap();
                        let wa = guard.iter().find(|w| w.id == webapp_id).cloned();
                        drop(guard);

                        if let Some(wa) = wa {
                            let label = format!("webapp_{}", webapp_id);
                            if let Some(win) = app.get_webview_window(&label) {
                                let _ = win.close();
                            } else {
                                open_webapp_window(app, &wa);
                            }
                            refresh_tray_menu(app);
                        }
                    }
                })
                .build(app)?;

            let id_str = tray.id().0.clone();
            *app.state::<TrayIdState>().lock().unwrap() = Some(id_str);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("errore durante l'avvio di Merlino")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
