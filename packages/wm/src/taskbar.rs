use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
  FindWindowExW, FindWindowW, ShowWindow, SW_HIDE, SW_SHOW,
};

/// Manages hiding and restoring the Windows taskbar and secondary
/// taskbars on multi-monitor setups.
///
/// All operations are non-destructive — the taskbar is hidden via
/// `ShowWindow` and restored on cleanup or crash recovery.
pub struct TaskbarManager {
  /// Whether the taskbar is currently hidden by us.
  is_hidden: bool,

  /// Shared flag checked by the keyboard hook to block the Windows
  /// key when the taskbar is hidden.
  pub block_win_key: Arc<AtomicBool>,
}

impl TaskbarManager {
  /// Creates a new `TaskbarManager`.
  pub fn new() -> Self {
    Self {
      is_hidden: false,
      block_win_key: Arc::new(AtomicBool::new(false)),
    }
  }

  /// Hides the taskbar, secondary taskbars, and enables Windows key
  /// blocking.
  pub fn hide(&mut self) {
    if self.is_hidden {
      return;
    }

    tracing::info!("Hiding Windows taskbar.");

    // Hide main taskbar.
    if let Some(hwnd) = find_window(w!("Shell_TrayWnd")) {
      // SAFETY: Hiding a system window we located via FindWindowW.
      unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
      }
    }

    // Hide secondary taskbars (multi-monitor).
    hide_all_secondary_taskbars();

    self.block_win_key.store(true, Ordering::Release);
    self.is_hidden = true;
  }

  /// Restores the taskbar and disables Windows key blocking.
  pub fn restore(&mut self) {
    if !self.is_hidden {
      return;
    }

    tracing::info!("Restoring Windows taskbar.");

    // Show main taskbar.
    if let Some(hwnd) = find_window(w!("Shell_TrayWnd")) {
      // SAFETY: Showing a system window we located via FindWindowW.
      unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
      }
    }

    // Show secondary taskbars.
    show_all_secondary_taskbars();

    self.block_win_key.store(false, Ordering::Release);
    self.is_hidden = false;
  }

  /// Re-hides the taskbar if it was supposed to be hidden. Called
  /// when `WM_SETTINGCHANGE` / `SPI_SETWORKAREA` fires, which can
  /// happen when explorer.exe restarts.
  pub fn reapply_if_hidden(&self) {
    if !self.is_hidden {
      return;
    }

    tracing::info!("Re-hiding taskbar after system change.");

    if let Some(hwnd) = find_window(w!("Shell_TrayWnd")) {
      // SAFETY: Re-hiding the taskbar window.
      unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
      }
    }

    hide_all_secondary_taskbars();
  }
}

impl Drop for TaskbarManager {
  fn drop(&mut self) {
    self.restore();
  }
}

/// Finds a top-level window by class name.
fn find_window(class_name: windows::core::PCWSTR) -> Option<HWND> {
  // SAFETY: FindWindowW with a valid class name string.
  let hwnd = unsafe { FindWindowW(class_name, None) };
  if hwnd.0 == 0 {
    None
  } else {
    Some(hwnd)
  }
}

/// Iterates all secondary taskbar windows and applies `action`.
fn for_each_secondary_taskbar(action: i32) {
  let class = w!("Shell_SecondaryTrayWnd");

  // SAFETY: Enumerating top-level windows by class name.
  let mut hwnd =
    unsafe { FindWindowExW(HWND::default(), HWND::default(), class, None) };

  while hwnd.0 != 0 {
    // SAFETY: Showing or hiding secondary taskbar windows.
    unsafe {
      let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD(action));
      hwnd = FindWindowExW(HWND::default(), hwnd, class, None);
    }
  }
}

/// Hides all secondary taskbar windows (multi-monitor).
fn hide_all_secondary_taskbars() {
  for_each_secondary_taskbar(SW_HIDE.0);
}

/// Shows all secondary taskbar windows.
fn show_all_secondary_taskbars() {
  for_each_secondary_taskbar(SW_SHOW.0);
}

/// Static helper to restore the taskbar without needing a
/// `TaskbarManager` instance.
#[allow(dead_code)]
pub fn restore_taskbar() {
  if let Some(hwnd) = find_window(w!("Shell_TrayWnd")) {
    unsafe {
      let _ = ShowWindow(hwnd, SW_SHOW);
    }
  }
  show_all_secondary_taskbars();
}
