#[derive(Debug, Clone)]
pub struct RunningApp {
    pub name: String,
    pub pid: i32,
}

pub fn get_running_gui_apps() -> Vec<RunningApp> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: *mut Object = msg_send![workspace, runningApplications];
        let count: usize = msg_send![apps, count];

        let mut result = Vec::new();
        for i in 0..count {
            let app: *mut Object = msg_send![apps, objectAtIndex: i];

            // NSApplicationActivationPolicyRegular = 0 (app con UI grafica)
            let policy: i64 = msg_send![app, activationPolicy];
            if policy != 0 {
                continue;
            }

            // Escludi Merlino stesso
            let bundle_id: *mut Object = msg_send![app, bundleIdentifier];
            if !bundle_id.is_null() {
                let id_ptr: *const c_char = msg_send![bundle_id, UTF8String];
                if !id_ptr.is_null() {
                    let id = CStr::from_ptr(id_ptr).to_string_lossy();
                    if id == "com.merlino.app" {
                        continue;
                    }
                }
            }

            let pid: i32 = msg_send![app, processIdentifier];

            let name_obj: *mut Object = msg_send![app, localizedName];
            let name = if !name_obj.is_null() {
                let name_ptr: *const c_char = msg_send![name_obj, UTF8String];
                if !name_ptr.is_null() {
                    CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
                } else {
                    format!("PID {}", pid)
                }
            } else {
                format!("PID {}", pid)
            };

            result.push(RunningApp { name, pid });
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }
}
