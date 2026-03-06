use std::ffi::CStr;
use std::os::raw::{c_char, c_void};

#[derive(Debug, Clone)]
pub struct RunningApp {
    pub name: String,
    pub pid: i32,
}

/// Lista le app GUI in esecuzione (activation policy = Regular, esclusa Merlino).
pub fn get_running_gui_apps() -> Vec<RunningApp> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: *mut Object = msg_send![workspace, runningApplications];
        let count: usize = msg_send![apps, count];

        let mut result = Vec::new();
        for i in 0..count {
            let app: *mut Object = msg_send![apps, objectAtIndex: i];

            let policy: i64 = msg_send![app, activationPolicy];
            if policy != 0 { continue; } // solo NSApplicationActivationPolicyRegular

            let bundle_id: *mut Object = msg_send![app, bundleIdentifier];
            if !bundle_id.is_null() {
                let id_ptr: *const c_char = msg_send![bundle_id, UTF8String];
                if !id_ptr.is_null() {
                    let id = CStr::from_ptr(id_ptr).to_string_lossy();
                    if id == "com.merlino.app" { continue; }
                }
            }

            let pid: i32 = msg_send![app, processIdentifier];
            let name_obj: *mut Object = msg_send![app, localizedName];
            let name = if !name_obj.is_null() {
                let name_ptr: *const c_char = msg_send![name_obj, UTF8String];
                if !name_ptr.is_null() {
                    CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
                } else { format!("PID {}", pid) }
            } else { format!("PID {}", pid) };

            result.push(RunningApp { name, pid });
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }
}

/// Porta in primo piano l'app con il PID indicato (usata al momento del pin).
pub fn activate_app(pid: i32) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: *mut Object = msg_send![workspace, runningApplications];
        let count: usize = msg_send![apps, count];

        for i in 0..count {
            let app: *mut Object = msg_send![apps, objectAtIndex: i];
            let app_pid: i32 = msg_send![app, processIdentifier];
            if app_pid == pid {
                let _: () = msg_send![app, activateWithOptions: 2u64]; // NSApplicationActivateIgnoringOtherApps
                break;
            }
        }
    }
}

// ── Accessibility API ─────────────────────────────────────────────────────────

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *mut *const c_void,
    ) -> i32;
    fn AXUIElementPerformAction(element: *const c_void, action: *const c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

/// Verifica se il permesso Accessibilità è già concesso (senza mostrare dialoghi).
pub fn is_accessibility_granted() -> bool {
    unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) }
}

/// Mostra il dialogo di sistema per richiedere il permesso Accessibilità.
/// Se l'app non è già in lista, macOS la aggiunge e mostra il toggle.
/// Restituisce true se il permesso è già concesso.
pub fn request_accessibility_permission() -> bool {
    use objc::runtime::{Object, YES};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        // NSDictionary @{ @"AXTrustedCheckOptionPrompt": @YES }
        let key: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: "AXTrustedCheckOptionPrompt\0".as_ptr() as *const c_char
        ];
        let value: *mut Object = msg_send![class!(NSNumber), numberWithBool: YES];
        let dict: *mut Object = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: value
            forKey: key
        ];
        AXIsProcessTrustedWithOptions(dict as *const c_void)
    }
}

/// Porta in primo piano la finestra principale dell'app via AX (senza rubare il focus tastiera).
/// Richiede che il permesso Accessibilità sia concesso.
pub fn raise_window_ax(pid: i32) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app_ref = AXUIElementCreateApplication(pid);
        if app_ref.is_null() { return; }

        // "AXWindows" come CFString (NSString è toll-free bridged con CFString)
        let ax_windows: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: "AXWindows\0".as_ptr() as *const c_char
        ];

        let mut windows_ref: *const c_void = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            app_ref,
            ax_windows as *const c_void,
            &mut windows_ref,
        );

        if err != 0 || windows_ref.is_null() {
            eprintln!("[merlino] raise_window_ax pid={}: AXWindows err={}", pid, err);
            CFRelease(app_ref);
            return;
        }

        let count = CFArrayGetCount(windows_ref);
        if count == 0 {
            eprintln!("[merlino] raise_window_ax pid={}: nessuna finestra AX", pid);
            CFRelease(windows_ref);
            CFRelease(app_ref);
            return;
        }

        let window = CFArrayGetValueAtIndex(windows_ref, 0);

        // "AXRaise": porta la finestra in primo piano senza rubare il focus tastiera
        let ax_raise: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: "AXRaise\0".as_ptr() as *const c_char
        ];
        let raise_err = AXUIElementPerformAction(window, ax_raise as *const c_void);
        eprintln!("[merlino] raise_window_ax pid={}: AXRaise={}", pid, raise_err);

        CFRelease(windows_ref);
        CFRelease(app_ref);
    }
}
