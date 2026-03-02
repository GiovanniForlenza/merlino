#![allow(non_upper_case_globals, non_snake_case)]

use std::os::raw::{c_int, c_void};

// Tipi CF opachi
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;

// CGWindowListOption
const kCGWindowListOptionAll: u32 = 0;
const kCGNullWindowID: u32 = 0;

// CFNumberType: SInt32 = 3
const kCFNumberSInt32Type: c_int = 3;

// kCGFloatingWindowLevel = 3 (sopra le finestre normali)
const kCGFloatingWindowLevel: c_int = 3;

// ── CoreGraphics ──────────────────────────────────────────────────────────────
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    // Chiavi del dizionario CGWindowList
    static kCGWindowNumber: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;

    fn CGWindowListCopyWindowInfo(option: u32, relativeToWindow: u32) -> CFArrayRef;

    // Connessione al window server (disponibile in CoreGraphics su macOS 11+)
    fn _CGSDefaultConnection() -> c_int;
}

// ── CoreFoundation ────────────────────────────────────────────────────────────
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: isize) -> CFTypeRef;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFStringRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, the_type: c_int, value_ptr: *mut c_void) -> bool;
    fn CFRelease(cf: CFTypeRef);
}

// ── SkyLight (private framework, macOS 11+) ───────────────────────────────────
#[link(name = "SkyLight", kind = "framework")]
extern "C" {
    fn SLSGetWindowLevel(cid: c_int, wid: u32, level: *mut c_int) -> c_int;
    fn SLSSetWindowLevel(cid: c_int, wid: u32, level: c_int) -> c_int;
}

// ── API pubblica del modulo ───────────────────────────────────────────────────

/// Restituisce il primo CGWindowID appartenente al PID indicato.
pub fn get_window_id_for_pid(pid: i32) -> Option<u32> {
    unsafe {
        let windows = CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID);
        if windows.is_null() {
            return None;
        }

        let count = CFArrayGetCount(windows);
        let mut result: Option<u32> = None;

        'outer: for i in 0..count {
            let dict = CFArrayGetValueAtIndex(windows, i);
            if dict.is_null() {
                continue;
            }

            // Legge il PID del proprietario della finestra
            let pid_ref = CFDictionaryGetValue(dict as CFDictionaryRef, kCGWindowOwnerPID);
            if pid_ref.is_null() {
                continue;
            }
            let mut owner_pid: c_int = 0;
            if !CFNumberGetValue(
                pid_ref,
                kCFNumberSInt32Type,
                &mut owner_pid as *mut c_int as *mut c_void,
            ) {
                continue;
            }
            if owner_pid != pid {
                continue;
            }

            // Legge il CGWindowNumber (= window ID)
            let wid_ref = CFDictionaryGetValue(dict as CFDictionaryRef, kCGWindowNumber);
            if wid_ref.is_null() {
                continue;
            }
            let mut window_id: c_int = 0;
            if CFNumberGetValue(
                wid_ref,
                kCFNumberSInt32Type,
                &mut window_id as *mut c_int as *mut c_void,
            ) {
                result = Some(window_id as u32);
                break 'outer;
            }
        }

        CFRelease(windows);
        result
    }
}

/// Legge il livello attuale di una finestra (per poterlo ripristinare al de-pin).
pub fn get_window_level(window_id: u32) -> i32 {
    unsafe {
        let cid = _CGSDefaultConnection();
        let mut level: c_int = 0;
        SLSGetWindowLevel(cid, window_id, &mut level);
        level
    }
}

/// Porta la finestra al livello floating (sempre in primo piano).
pub fn pin_window(window_id: u32) -> Result<(), String> {
    unsafe {
        let cid = _CGSDefaultConnection();
        let ret = SLSSetWindowLevel(cid, window_id, kCGFloatingWindowLevel);
        if ret == 0 {
            Ok(())
        } else {
            Err(format!("SLSSetWindowLevel failed: {}", ret))
        }
    }
}

/// Ripristina il livello originale della finestra.
pub fn unpin_window(window_id: u32, original_level: i32) -> Result<(), String> {
    unsafe {
        let cid = _CGSDefaultConnection();
        let ret = SLSSetWindowLevel(cid, window_id, original_level as c_int);
        if ret == 0 {
            Ok(())
        } else {
            Err(format!("SLSSetWindowLevel failed: {}", ret))
        }
    }
}
