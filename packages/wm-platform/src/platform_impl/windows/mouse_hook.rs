use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::{
  Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
  UI::{
    Input::KeyboardAndMouse::{GetKeyState, VK_LMENU, VK_RMENU},
    WindowsAndMessaging::{
      CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
      WH_MOUSE_LL, WM_RBUTTONDOWN, WM_RBUTTONUP,
    },
  },
};

use crate::Dispatcher;

/// When true, an alt-drag resize is in progress and right-click
/// events should be blocked from reaching windows.
static RESIZE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// A system-wide low-level mouse hook that blocks right-click events
/// from reaching target windows during alt-drag resize.
#[derive(Debug)]
pub struct AltClickMouseHook {
  handle: HHOOK,
  dispatcher: Dispatcher,
}

impl AltClickMouseHook {
  /// Creates and registers the mouse hook on the event loop thread.
  pub fn new(dispatcher: &Dispatcher) -> crate::Result<Self> {
    let handle = dispatcher.dispatch_sync(|| unsafe {
      SetWindowsHookExW(
        WH_MOUSE_LL,
        Some(Self::hook_proc),
        HINSTANCE::default(),
        0,
      )
    })??;

    Ok(Self {
      handle,
      dispatcher: dispatcher.clone(),
    })
  }

  /// Sets whether alt-drag resize is currently active. When active,
  /// right-click events are blocked from reaching windows.
  pub fn set_resize_active(active: bool) {
    RESIZE_ACTIVE.store(active, Ordering::Release);
  }

  /// Terminates the mouse hook.
  pub fn terminate(&mut self) -> crate::Result<()> {
    unsafe { UnhookWindowsHookEx(self.handle) }?;
    Ok(())
  }

  /// Hook procedure that intercepts right-click during active resize.
  extern "system" fn hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
  ) -> LRESULT {
    if code != 0 {
      return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    #[allow(clippy::cast_possible_truncation)]
    let msg = wparam.0 as u32;

    // During active resize, block right-click from reaching windows.
    if RESIZE_ACTIVE.load(Ordering::Acquire)
      && (msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP)
    {
      return LRESULT(1);
    }

    // Block right-click when Alt is held and no resize is active yet
    // (the initial click that starts the resize). Only block the UP
    // event — the DOWN event needs to reach our raw input listener
    // to start the drag.
    if msg == WM_RBUTTONUP {
      let alt_down = unsafe {
        (GetKeyState(VK_LMENU.0.into()) & 0x80 == 0x80)
          || (GetKeyState(VK_RMENU.0.into()) & 0x80 == 0x80)
      };

      if alt_down {
        return LRESULT(1);
      }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
  }
}

impl Drop for AltClickMouseHook {
  fn drop(&mut self) {
    let _ = self.terminate();
  }
}
