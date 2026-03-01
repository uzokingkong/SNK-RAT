use crate::config::{DecoyConfig, MessageBoxIcon, MessageBoxButtons};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use winapi::um::winuser::{
    MessageBoxW, MB_OK, MB_ICONERROR, MB_OKCANCEL, MB_ICONWARNING, MB_ICONINFORMATION,
    MB_ICONQUESTION, MB_YESNO, MB_TOPMOST, MB_SETFOREGROUND, MB_TASKMODAL,
};

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

pub fn show_fake_error(config: &DecoyConfig) {
    if !config.enabled {
        return;
    }

    let title_wide = to_wide(&config.title);
    let message_wide = to_wide(&config.message);

    // map our simple enum to ugly winapi flags
    let icon_flag = match config.icon {
        MessageBoxIcon::Error => MB_ICONERROR,
        MessageBoxIcon::Warning => MB_ICONWARNING,
        MessageBoxIcon::Info => MB_ICONINFORMATION,
        MessageBoxIcon::Question => MB_ICONQUESTION,
    };

    let button_flag = match config.buttons {
        MessageBoxButtons::Ok => MB_OK,
        MessageBoxButtons::OkCancel => MB_OKCANCEL,
        MessageBoxButtons::YesNo => MB_YESNO,
    };

    // this is a blocking call, must be run in a separate thread
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            icon_flag | button_flag | MB_TOPMOST | MB_SETFOREGROUND | MB_TASKMODAL,
        );
    }
}
