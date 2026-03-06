//! PiP overlay: finestra Tauri floating (always-on-top) che mostra il contenuto
//! live di un'altra app via CGWindowListCreateImage a ~30fps.
//! I click passano attraverso la finestra all'app sottostante (ignores_cursor_events).

#![allow(non_upper_case_globals, non_snake_case)]

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

// ── Tipi CG ──────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CGPoint {
    pub x: c_double,
    pub y: c_double,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CGSize {
    pub width: c_double,
    pub height: c_double,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

// ── Screen Recording permission ───────────────────────────────────────────────

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

pub fn has_screen_recording_permission() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}
pub fn request_screen_recording_permission() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

// ── Window bounds (CG coords: origine top-left) ───────────────────────────────

type CFTypeRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relativeToWindow: u32) -> CFArrayRef;
    static kCGWindowOwnerPID: CFStringRef;
    static kCGWindowBounds: CFStringRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(arr: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: isize) -> CFTypeRef;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFStringRef) -> CFTypeRef;
    fn CFNumberGetValue(num: CFTypeRef, the_type: std::os::raw::c_int, out: *mut c_void) -> bool;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect) -> bool;
    #[link_name = "CFRelease"]
    fn cf_release(cf: CFTypeRef);
}

/// Bounds della prima finestra on-screen del PID in screen coordinates (origine top-left).
pub fn get_window_bounds_cg(pid: i32) -> Option<CGRect> {
    unsafe {
        let list = CGWindowListCopyWindowInfo(1 /* onScreenOnly */, 0);
        if list.is_null() {
            return None;
        }
        let n = CFArrayGetCount(list);
        let mut result = None;

        for i in 0..n {
            let dict = CFArrayGetValueAtIndex(list, i) as CFDictionaryRef;
            if dict.is_null() {
                continue;
            }

            let pid_v = CFDictionaryGetValue(dict, kCGWindowOwnerPID);
            if pid_v.is_null() {
                continue;
            }
            let mut p: c_int = 0;
            if !CFNumberGetValue(pid_v, 3 /* SInt32 */, &mut p as *mut c_int as *mut c_void) {
                continue;
            }
            if p != pid {
                continue;
            }

            let bounds_v = CFDictionaryGetValue(dict, kCGWindowBounds);
            if bounds_v.is_null() {
                continue;
            }
            let mut rect = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: 0.0, height: 0.0 },
            };
            if CGRectMakeWithDictionaryRepresentation(bounds_v as CFDictionaryRef, &mut rect) {
                result = Some(rect);
                break;
            }
        }

        cf_release(list);
        result
    }
}

// ── Cattura finestra via ScreenCaptureKit ─────────────────────────────────────

type CGImageRef = *mut c_void;

extern "C" {
    fn sck_init_capture(window_id: u32) -> std::os::raw::c_int;
    fn sck_capture_window(window_id: u32) -> CGImageRef;
    fn sck_stop_capture(window_id: u32);
}

fn capture_window(window_id: u32) -> Option<CGImageRef> {
    let img = unsafe { sck_capture_window(window_id) };
    if img.is_null() { None } else { Some(img) }
}

/// CGImageRef → Vec<u8> JPEG via NSBitmapImageRep (ImageIO nativo, nessun crate).
fn cgimage_to_jpeg(cg_image: CGImageRef) -> Option<Vec<u8>> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        // NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithCGImage:cg]
        let rep: *mut Object = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut Object = msg_send![rep, initWithCGImage: cg_image as *mut Object];
        if rep.is_null() {
            return None;
        }

        // NSDictionary *props = @{NSImageCompressionFactor: @0.65}
        let key: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: b"NSImageCompressionFactor\0".as_ptr() as *const c_char
        ];
        let val: *mut Object = msg_send![class!(NSNumber), numberWithDouble: 0.3f64];
        let props: *mut Object = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: val forKey: key
        ];

        // NSData *data = [rep representationUsingType:NSBitmapImageFileTypeJPEG properties:props]
        // NSBitmapImageFileTypeJPEG = 1
        let data: *mut Object =
            msg_send![rep, representationUsingType: 1usize properties: props];

        let result = if !data.is_null() {
            let len: usize = msg_send![data, length];
            let bytes: *const u8 = msg_send![data, bytes];
            Some(std::slice::from_raw_parts(bytes, len).to_vec())
        } else {
            None
        };

        let _: () = msg_send![rep, release];
        result
    }
}

// ── Altezza schermo principale (Cocoa coords) ─────────────────────────────────

fn main_screen_height() -> f64 {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    #[repr(C)]
    struct NsRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    unsafe {
        let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
        let frame: NsRect = msg_send![screen, frame];
        frame.h
    }
}

// ── API pubblica ──────────────────────────────────────────────────────────────

/// Avvia la sessione PiP per il PID indicato.
/// - Controlla il permesso Screen Recording (richiede se mancante)
/// - Crea una finestra Tauri floating alla posizione/dimensione della finestra originale
/// - Avvia un thread che cattura screenshot a ~30fps e li emette alla finestra
/// - Restituisce un AtomicBool per fermare il thread (store false per stopparla)
pub fn start_pip(
    app: &AppHandle,
    pid: i32,
    _window_id: u32,
) -> Result<Arc<AtomicBool>, String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    if !has_screen_recording_permission() {
        request_screen_recording_permission();
        return Err(
            "Permesso Screen Recording mancante. \
             Abilita Merlino in System Settings → Privacy → Registrazione schermo, \
             poi riavvia l'app."
                .to_string(),
        );
    }

    let bounds = get_window_bounds_cg(pid)
        .ok_or_else(|| format!("Nessuna finestra on-screen per PID {}", pid))?;

    // Converti CG coords (top-left origin) → Cocoa coords (bottom-left origin)
    let screen_h = main_screen_height();
    let cocoa_y = screen_h - bounds.origin.y - bounds.size.height;

    let label = format!("pip_{}", pid);

    // Chiudi eventuale sessione precedente
    if let Some(old) = app.get_webview_window(&label) {
        let _ = old.close();
    }

    let _win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(format!("pip.html?pid={}", pid).into()))
        .title("")
        .always_on_top(true)
        .decorations(false)
        .resizable(true)
        .position(bounds.origin.x, cocoa_y)
        .inner_size(bounds.size.width, bounds.size.height)
        .build()
        .map_err(|e| format!("Impossibile creare finestra PiP: {}", e))?;

    // Inizializza il filtro SCK (lento, ~1-3s) — una sola volta qui
    let init_wid = crate::window_manager::get_window_id_for_pid(pid)
        .ok_or_else(|| format!("Nessuna finestra per PID {}", pid))?;
    let ok = unsafe { sck_init_capture(init_wid) };
    if ok == 0 {
        return Err(format!("sck_init_capture fallito per PID {}", pid));
    }
    eprintln!("[pip] SCK filtro inizializzato per PID {} wid={}", pid, init_wid);

    // Frame state condiviso (pip:// protocol handler legge da qui)
    let frame_state = app.state::<crate::FrameState>().inner().clone();

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_handle = app.clone();

    std::thread::spawn(move || {
        eprintln!("[pip] thread avviato per PID {}", pid);
        let mut n: u64 = 0;
        while running_clone.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50)); // ~20fps

            let wid = match crate::window_manager::get_window_id_for_pid(pid) {
                Some(w) => w,
                None => {
                    if n % 20 == 0 { eprintln!("[pip] pid={} no window id", pid); }
                    n += 1; continue;
                }
            };

            let img = match capture_window(wid) {
                Some(i) => i,
                None => { n += 1; continue; }
            };

            let jpeg = match cgimage_to_jpeg(img) {
                Some(j) => j,
                None => {
                    unsafe { cf_release(img as CFTypeRef) };
                    n += 1; continue;
                }
            };
            unsafe { cf_release(img as CFTypeRef) };

            if n % 20 == 0 {
                eprintln!("[pip] pid={} frame={} jpeg={}KB", pid, n, jpeg.len() / 1024);
            }
            n += 1;

            // Aggiorna il frame nello stato condiviso (letto da pip:// protocol, niente IPC)
            frame_state.lock().unwrap().insert(pid, jpeg);
        }
        unsafe { sck_stop_capture(init_wid) };
        frame_state.lock().unwrap().remove(&pid);
        if let Some(win) = app_handle.get_webview_window(&format!("pip_{}", pid)) {
            let _ = win.close();
        }
        eprintln!("[pip] thread terminato per PID {}", pid);
    });

    eprintln!(
        "[merlino] PiP avviato per PID {} at ({}, {}) {}x{}",
        pid,
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height
    );
    Ok(running)
}

/// Ferma la sessione PiP per il PID indicato.
pub fn stop_pip(running: &Arc<AtomicBool>) {
    running.store(false, Ordering::Relaxed);
}
