use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, IsWindowVisible, ShowWindow, SW_MINIMIZE,
};

pub fn minimize_all_windows() {
    unsafe {
        // Enumerate all windows and apply callback
        let _ = EnumWindows(Some(enum_window_callback), LPARAM(0));
    }
}

unsafe extern "system" fn enum_window_callback(hwnd: HWND, _: LPARAM) -> BOOL {
    // Check if the window is visible
    if IsWindowVisible(hwnd).as_bool() {
        // Minimize the window
        ShowWindow(hwnd, SW_MINIMIZE);
    }
    // Continue enumeration (TRUE)
    BOOL::from(true)
}
