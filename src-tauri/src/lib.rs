#[cfg(target_os = "macos")]
mod apps;
#[cfg(target_os = "macos")]
mod window_manager;
#[cfg(target_os = "macos")]
mod pip;

#[cfg(target_os = "macos")]
use apps::{get_running_gui_apps, raise_window_ax};
#[cfg(target_os = "macos")]
use window_manager::get_window_id_for_pid;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

pub struct PinnedEntry {
    pub name: String,
    pub pid: i32,
    pub window_id: u32,
    pub original_level: i32,
    /// Handle per fermare il thread PiP (None se PiP non è attivo)
    #[cfg(target_os = "macos")]
    pub pip_running: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

pub type PinnedApps = Arc<Mutex<HashMap<i32, PinnedEntry>>>;
pub type TrayIdState = Arc<Mutex<Option<String>>>;

/// Ultimo frame JPEG per ogni PID pinnato (condiviso tra thread capture e handler pip://)
pub type FrameState = Arc<Mutex<HashMap<i32, Vec<u8>>>>;

// Puntatore grezzo alla HashMap condivisa, usato dal callback ObjC (vive per tutta l'app).
static PINNED_PTR: AtomicUsize = AtomicUsize::new(0);

// Menu

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

fn refresh_tray_menu(app: &AppHandle) {
    let tray_id = app.state::<TrayIdState>().lock().unwrap().clone();
    if let Some(id) = tray_id {
        if let Some(tray) = app.tray_by_id(&id) {
            let pinned = app.state::<PinnedApps>().inner().clone();
            if let Ok(menu) = build_tray_menu(app, &pinned) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }
}

// Watcher event-driven (NSWorkspaceDidActivateApplicationNotification) 
//
// Quando l'utente attiva un'altra app, il callback ObjC ri-porta in primo piano
// tutte le finestre pinnate usando kAXRaiseAction (senza rubare il focus tastiera).

#[cfg(target_os = "macos")]
fn setup_activation_watcher(pinned: &PinnedApps) {
    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};
    use std::os::raw::c_char;

    // Callback invocato da NSNotificationCenter sul main thread
    extern "C" fn on_app_activated(_this: &Object, _cmd: Sel, _notif: *mut Object) {
        let ptr = PINNED_PTR.load(Ordering::Acquire);
        if ptr == 0 { return; }

        let map = unsafe { &*(ptr as *const Mutex<HashMap<i32, PinnedEntry>>) };
        let guard = match map.lock() {
            Ok(g) => g,
            Err(_) => return,
        };

        let pids: Vec<i32> = guard.values().map(|e| e.pid).collect();
        drop(guard);

        eprintln!("[merlino] app_activated, pids pinnati: {:?}", pids);

        if pids.is_empty() { return; }

        for pid in pids {
            raise_window_ax(pid);
        }
    }

    unsafe {
        // Registra la classe ObjC (una sola volta per processo)
        let observer: *mut Object = if let Some(cls) = Class::get("MerlinoActivationObserver") {
            msg_send![cls, new]
        } else {
            let superclass = Class::get("NSObject").unwrap();
            let mut decl = ClassDecl::new("MerlinoActivationObserver", superclass).unwrap();
            decl.add_method(
                sel!(onAppActivated:),
                on_app_activated as extern "C" fn(&Object, Sel, *mut Object),
            );
            let cls = decl.register();
            msg_send![cls, new]
        };

        // Registra l'observer con NSWorkspace.notificationCenter
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let nc: *mut Object = msg_send![workspace, notificationCenter];

        let notif_name: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String:
                "NSWorkspaceDidActivateApplicationNotification\0".as_ptr() as *const c_char
        ];

        let _: () = msg_send![nc,
            addObserver: observer
            selector: sel!(onAppActivated:)
            name: notif_name
            object: std::ptr::null::<Object>()
        ];

        // L'observer deve sopravvivere per tutta la durata dell'app
        std::mem::forget(observer);
    }

    // Salva il puntatore alla HashMap (dentro l'Arc, gestita da Tauri state)
    let raw = Arc::as_ptr(pinned) as usize;
    PINNED_PTR.store(raw, Ordering::Release);

    // Timer ogni 200ms: garantisce che le finestre pinnate restino in primo piano
    // anche quando macOS vince il race condition post-notifica.
    // Costo energetico: ~0.01% CPU (sleep + 1 chiamata AX ogni 200ms).
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));

            let ptr = PINNED_PTR.load(Ordering::Acquire);
            if ptr == 0 { continue; }

            let map = unsafe { &*(ptr as *const Mutex<HashMap<i32, PinnedEntry>>) };
            let pids: Vec<i32> = match map.lock() {
                Ok(g) => g.values().map(|e| e.pid).collect(),
                Err(_) => continue,
            };

            for pid in pids {
                raise_window_ax(pid);
            }
        }
    });
}

// Setup dell'app

pub fn run() {
    let pinned_apps: PinnedApps = Arc::new(Mutex::new(HashMap::new()));
    let tray_id_state: TrayIdState = Arc::new(Mutex::new(None));
    let frame_state: FrameState = Arc::new(Mutex::new(HashMap::new()));
    let frames_for_protocol = frame_state.clone();

    tauri::Builder::default()
        .manage(pinned_apps)
        .manage(tray_id_state)
        .manage(frame_state)
        .register_uri_scheme_protocol("pip", move |_ctx, request| {
            let path = request.uri().path(); // "/frame/12345"
            let pid: i32 = path
                .strip_prefix("/frame/")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let guard = frames_for_protocol.lock().unwrap();
            match guard.get(&pid) {
                Some(jpeg) => tauri::http::Response::builder()
                    .status(200)
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", "no-store")
                    .body(jpeg.clone())
                    .unwrap(),
                None => tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::<u8>::new())
                    .unwrap(),
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let icon = Image::new(include_bytes!("../icons/tray.rgba"), 32, 32);

            let pinned = app.state::<PinnedApps>().inner().clone();
            let initial_menu = build_tray_menu(app.handle(), &pinned)?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .menu(&initial_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app: &AppHandle, event| {
                    let id = event.id.as_ref();

                    if id == "quit" {
                        let pinned = app.state::<PinnedApps>().inner().clone();
                        let mut guard = pinned.lock().unwrap();
                        #[cfg(target_os = "macos")]
                        for entry in guard.values() {
                            if let Some(ref r) = entry.pip_running {
                                pip::stop_pip(r);
                            }
                        }
                        guard.clear();
                        drop(guard);
                        app.exit(0);
                    } else if let Some(pid_str) = id.strip_prefix("pin_") {
                        if let Ok(pid) = pid_str.parse::<i32>() {
                            handle_pin_toggle(app, pid);
                            refresh_tray_menu(app);
                        }
                    }
                })
                .build(app)?;

            let id_str = tray.id().0.clone();
            *app.state::<TrayIdState>().lock().unwrap() = Some(id_str);

            // Controlla il permesso Accessibilità
            #[cfg(target_os = "macos")]
            {
                let pinned = app.state::<PinnedApps>().inner().clone();

                // request_accessibility_permission() mostra il dialogo di sistema se non già concesso
                let granted = apps::request_accessibility_permission();
                eprintln!("[merlino] Accessibilità concessa: {}", granted);
                if granted {
                    eprintln!("[merlino] Avvio activation watcher...");
                    setup_activation_watcher(&pinned);
                    eprintln!("[merlino] Activation watcher attivo.");
                } else {
                    eprintln!(
                        "[merlino] PERMESSO MANCANTE — apri System Settings → Privacy → \
                         Accessibilità, abilita il toggle per 'merlino', poi RIAVVIA l'app."
                    );
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("errore durante l'avvio di Merlino");
}

// Pin / Unpin 

fn handle_pin_toggle(app: &AppHandle, pid: i32) {
    let pinned = app.state::<PinnedApps>().inner().clone();
    let mut guard = pinned.lock().unwrap();

    if let Some(entry) = guard.remove(&pid) {
        // DE-PIN: ferma il PiP
        #[cfg(target_os = "macos")]
        if let Some(ref running) = entry.pip_running {
            pip::stop_pip(running);
        }
    } else {
        // PIN: avvia il PiP
        #[cfg(target_os = "macos")]
        {
            let Some(wid) = get_window_id_for_pid(pid) else {
                eprintln!("[merlino] Nessuna finestra on-screen per PID {}", pid);
                return;
            };

            let name = get_running_gui_apps()
                .into_iter()
                .find(|a| a.pid == pid)
                .map(|a| a.name)
                .unwrap_or_else(|| format!("PID {}", pid));

            // Rilascia il lock prima di chiamare start_pip (che crea una finestra)
            drop(guard);

            let pip_running = match pip::start_pip(app, pid, wid) {
                Ok(r) => {
                    eprintln!("[merlino] PiP avviato per '{}' (PID {})", name, pid);
                    Some(r)
                }
                Err(e) => {
                    eprintln!("[merlino] Errore PiP: {}", e);
                    None
                }
            };

            // Riacquisisci il lock e inserisci
            let mut guard2 = pinned.lock().unwrap();
            guard2.insert(
                pid,
                PinnedEntry {
                    name,
                    pid,
                    window_id: wid,
                    original_level: 0,
                    pip_running,
                },
            );
            return; // guard2 viene droppato qui
        }
        #[cfg(not(target_os = "macos"))]
        let _ = pid;
    }
}
