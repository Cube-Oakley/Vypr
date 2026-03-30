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

/// Tracks whether the right mouse button is physically held, even
/// when the hook blocks the message from reaching windows.
static RBUTTON_DOWN: AtomicBool = AtomicBool::new(false);

/// A system-wide low-level mouse hook that blocks right-click events
/// from reaching windows when Alt is held, while tracking the button
/// state so `is_rbutton_down()` still works.
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

  /// Returns whether the right mouse button is currently held,
  /// tracked by the hook even when messages are blocked.
  pub fn is_rbutton_down() -> bool {
    RBUTTON_DOWN.load(Ordering::Acquire)
  }

  /// Terminates the mouse hook.
  pub fn terminate(&mut self) -> crate::Result<()> {
    unsafe { UnhookWindowsHookEx(self.handle) }?;
    Ok(())
  }

  /// Hook procedure that blocks right-click when Alt is held and
  /// tracks button state.
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

    // Track right button state regardless of blocking.
    if msg == WM_RBUTTONDOWN {
      RBUTTON_DOWN.store(true, Ordering::Release);
    } else if msg == WM_RBUTTONUP {
      RBUTTON_DOWN.store(false, Ordering::Release);
    }

    // Block right-click from reaching windows when Alt is held.
    if (msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP)
      && is_alt_down()
    {
      return LRESULT(1);
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
  }
}

/// Checks if Alt is currently held.
fn is_alt_down() -> bool {
  unsafe {
    (GetKeyState(VK_LMENU.0.into()) & 0x80 == 0x80)
      || (GetKeyState(VK_RMENU.0.into()) & 0x80 == 0x80)
  }
}

impl Drop for AltClickMouseHook {
  fn drop(&mut self) {
    let _ = self.terminate();
  }
}
