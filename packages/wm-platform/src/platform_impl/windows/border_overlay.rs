use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
  CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
  SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
  BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
  SetWindowPos, ShowWindow, UpdateLayeredWindow, ULW_ALPHA, WNDCLASSW,
  HWND_TOP, SWP_NOACTIVATE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE,
  SW_SHOWNOACTIVATE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
  WS_POPUP,
};

use crate::{Color, Dispatcher, Rect};

/// Window procedure for overlay windows. Delegates all messages to
/// the default handler.
unsafe extern "system" fn overlay_wnd_proc(
  hwnd: HWND,
  msg: u32,
  wparam: windows::Win32::Foundation::WPARAM,
  lparam: windows::Win32::Foundation::LPARAM,
) -> LRESULT {
  // SAFETY: Delegating to the default window procedure.
  unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Registers the overlay window class (idempotent, thread-safe).
fn ensure_class_registered() {
  if CLASS_REGISTERED.load(Ordering::Acquire) {
    return;
  }

  let class = WNDCLASSW {
    lpszClassName: w!("VyprBorderOverlay"),
    lpfnWndProc: Some(overlay_wnd_proc),
    ..Default::default()
  };

  // SAFETY: Registering a window class with a valid struct.
  unsafe {
    RegisterClassW(&raw const class);
  }
  CLASS_REGISTERED.store(true, Ordering::Release);
}

/// Manages gradient border overlay windows for all bordered windows.
///
/// Overlay windows are created on the event loop thread via the
/// `Dispatcher` to ensure they receive Win32 messages properly.
pub struct BorderOverlayManager {
  /// Maps target window handle to its overlay HWND (as raw isize).
  overlays: HashMap<isize, OverlayState>,
}

struct OverlayState {
  /// Raw HWND handle (isize) so we can send it across threads.
  handle: isize,
  /// Cached dimensions to skip re-rendering on position-only changes.
  width: i32,
  height: i32,
}

impl BorderOverlayManager {
  /// Creates a new `BorderOverlayManager`.
  pub fn new() -> Self {
    Self {
      overlays: HashMap::new(),
    }
  }

  /// Creates or updates the gradient border overlay for a target
  /// window. Window creation and rendering are dispatched to the
  /// event loop thread.
  pub fn update(
    &mut self,
    target_handle: isize,
    frame: &Rect,
    colors: &[Color],
    angle_deg: f64,
    border_width: u32,
    dispatcher: &Dispatcher,
  ) -> crate::Result<()> {
    let bw = border_width as i32;
    let overlay_x = frame.x() - bw;
    let overlay_y = frame.y() - bw;
    let overlay_w = frame.width() + 2 * bw;
    let overlay_h = frame.height() + 2 * bw;

    if overlay_w <= 0 || overlay_h <= 0 {
      return Ok(());
    }

    let needs_create = !self.overlays.contains_key(&target_handle);
    let size_changed = self
      .overlays
      .get(&target_handle)
      .is_none_or(|s| s.width != overlay_w || s.height != overlay_h);

    if needs_create {
      // Create the overlay window on the event loop thread.
      let overlay_handle = dispatcher.dispatch_sync(move || {
        ensure_class_registered();
        create_overlay_hwnd(overlay_x, overlay_y, overlay_w, overlay_h)
      })??;

      self.overlays.insert(
        target_handle,
        OverlayState {
          handle: overlay_handle,
          width: overlay_w,
          height: overlay_h,
        },
      );
    }

    let state = self.overlays.get_mut(&target_handle).ok_or_else(|| {
      crate::Error::Platform("Overlay not found.".to_string())
    })?;

    if size_changed || needs_create {
      // Render and position on the event loop thread (async to
      // avoid blocking the WM thread during focus changes).
      let overlay_h_copy = state.handle;
      let colors = colors.to_vec();
      dispatcher.dispatch_async(move || {
        let hwnd = HWND(overlay_h_copy);
        if let Err(e) = render_gradient(
          hwnd, overlay_x, overlay_y, overlay_w, overlay_h, bw, &colors,
          angle_deg,
        ) {
          tracing::warn!("Failed to render border gradient: {e}");
        }

        // SAFETY: Positioning the overlay above the target window.
        unsafe {
          let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            overlay_x,
            overlay_y,
            overlay_w,
            overlay_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
          );
        }
      })?;

      state.width = overlay_w;
      state.height = overlay_h;
    } else {
      // Position-only update (async).
      let overlay_h_copy = state.handle;
      dispatcher.dispatch_async(move || {
        // SAFETY: Repositioning an existing overlay window.
        unsafe {
          let _ = SetWindowPos(
            HWND(overlay_h_copy),
            HWND_TOP,
            overlay_x,
            overlay_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
          );
        }
      })?;
    }

    Ok(())
  }

  /// Destroys the overlay for a target window.
  pub fn destroy(
    &mut self,
    target_handle: isize,
    dispatcher: &Dispatcher,
  ) {
    if let Some(state) = self.overlays.remove(&target_handle) {
      let h = state.handle;
      let _ = dispatcher.dispatch_async(move || {
        // SAFETY: Destroying a window we created.
        unsafe {
          let _ = DestroyWindow(HWND(h));
        }
      });
    }
  }

  /// Destroys all overlays.
  pub fn destroy_all(&mut self, dispatcher: &Dispatcher) {
    let handles: Vec<_> = self.overlays.keys().copied().collect();
    for handle in handles {
      self.destroy(handle, dispatcher);
    }
  }

  /// Hides the overlay for a target window.
  pub fn hide(
    &mut self,
    target_handle: isize,
    dispatcher: &Dispatcher,
  ) {
    if let Some(state) = self.overlays.get(&target_handle) {
      let h = state.handle;
      let _ = dispatcher.dispatch_async(move || {
        // SAFETY: Hiding a window we created.
        unsafe {
          let _ = ShowWindow(HWND(h), SW_HIDE);
        }
      });
    }
  }

  /// Shows the overlay for a target window.
  pub fn show(
    &mut self,
    target_handle: isize,
    dispatcher: &Dispatcher,
  ) {
    if let Some(state) = self.overlays.get(&target_handle) {
      let h = state.handle;
      let _ = dispatcher.dispatch_async(move || {
        // SAFETY: Showing a window we created.
        unsafe {
          let _ = ShowWindow(HWND(h), SW_SHOWNOACTIVATE);
        }
      });
    }
  }

  /// Returns true if an overlay exists for the given target.
  pub fn has_overlay(&self, target_handle: isize) -> bool {
    self.overlays.contains_key(&target_handle)
  }
}

/// Creates the overlay HWND on the current thread.
///
/// Must be called on the event loop thread.
fn create_overlay_hwnd(
  x: i32,
  y: i32,
  w: i32,
  h: i32,
) -> crate::Result<isize> {
  let ex_style =
    WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;

  // SAFETY: Creating a popup window with valid parameters.
  let hwnd = unsafe {
    CreateWindowExW(
      ex_style,
      w!("VyprBorderOverlay"),
      w!(""),
      WS_POPUP,
      x,
      y,
      w,
      h,
      None,
      None,
      None,
      None,
    )
  };

  if hwnd.0 == 0 {
    return Err(crate::Error::Platform(
      "Failed to create overlay window.".to_string(),
    ));
  }

  Ok(hwnd.0)
}

/// Renders the gradient border into a DIB section and composites it
/// onto the overlay window via `UpdateLayeredWindow`.
///
/// Must be called on the event loop thread.
fn render_gradient(
  hwnd: HWND,
  x: i32,
  y: i32,
  w: i32,
  h: i32,
  border_width: i32,
  colors: &[Color],
  angle_deg: f64,
) -> crate::Result<()> {
  // SAFETY: Creating a memory DC for offscreen rendering.
  let hdc = unsafe { CreateCompatibleDC(None) };

  let bmi = BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER {
      biSize: size_of::<BITMAPINFOHEADER>() as u32,
      biWidth: w,
      // Negative height = top-down DIB.
      biHeight: -h,
      biPlanes: 1,
      biBitCount: 32,
      biCompression: BI_RGB.0 as u32,
      ..Default::default()
    },
    ..Default::default()
  };

  let mut bits: *mut std::ffi::c_void = ptr::null_mut();

  // SAFETY: Creating a DIB section for pixel buffer rendering.
  let hbitmap = unsafe {
    CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
  }
  .map_err(|e| crate::Error::Platform(e.to_string()))?;

  // SAFETY: Selecting the bitmap into the DC.
  let old_bitmap = unsafe { SelectObject(hdc, hbitmap) };

  // Fill pixel buffer.
  let pixel_count = (w * h) as usize;
  // SAFETY: The DIB section allocated `pixel_count` u32 pixels.
  let pixels: &mut [u32] = unsafe {
    std::slice::from_raw_parts_mut(bits.cast::<u32>(), pixel_count)
  };

  let angle_rad = angle_deg.to_radians();
  let dx = angle_rad.sin();
  let dy = -angle_rad.cos();

  // Windows 11 window corner radius (~8px). The outer edge of the
  // border uses a slightly larger radius so the border follows the
  // window's rounded shape.
  let inner_radius = 8.0_f64;
  let outer_radius = inner_radius + border_width as f64;

  for py in 0..h {
    for px in 0..w {
      // Check if pixel is inside the outer rounded rect but outside
      // the inner rounded rect (i.e., in the border region).
      let in_outer = is_inside_rounded_rect(
        px, py, 0, 0, w, h, outer_radius,
      );
      // Shrink the inner cutout by 1px so the border overlaps the
      // window edge slightly. This covers the small gap between the
      // DWM frame bounds and the visible window edge on some apps.
      let inset = 1;
      let in_inner = is_inside_rounded_rect(
        px,
        py,
        border_width + inset,
        border_width + inset,
        w - border_width - inset,
        h - border_width - inset,
        inner_radius,
      );

      if in_outer && !in_inner {
        let t = gradient_param(px, py, w, h, dx, dy);
        let color = sample_gradient(colors, t);
        pixels[(py * w + px) as usize] = color.to_premultiplied_argb();
      }
      // else: pixel stays 0 (fully transparent).
    }
  }

  let pt_pos = POINT { x, y };
  let sz = SIZE { cx: w, cy: h };
  let pt_src = POINT { x: 0, y: 0 };
  let blend = BLENDFUNCTION {
    BlendOp: AC_SRC_OVER as u8,
    BlendFlags: 0,
    SourceConstantAlpha: 255,
    AlphaFormat: AC_SRC_ALPHA as u8,
  };

  // SAFETY: Compositing the rendered bitmap onto the layered window.
  let result = unsafe {
    UpdateLayeredWindow(
      hwnd,
      None,
      Some(&pt_pos),
      Some(&sz),
      hdc,
      Some(&pt_src),
      COLORREF(0),
      Some(&blend),
      ULW_ALPHA,
    )
  };

  if let Err(ref e) = result {
    tracing::warn!("UpdateLayeredWindow failed: {e}");
  }

  // SAFETY: Clean up GDI objects.
  unsafe {
    SelectObject(hdc, old_bitmap);
    let _ = DeleteObject(hbitmap);
    let _ = DeleteDC(hdc);
  }

  result.map_err(|e| crate::Error::Platform(e.to_string()))
}

/// Computes the gradient parameter (0.0..=1.0) for a pixel position
/// based on the gradient direction vector.
fn gradient_param(
  px: i32,
  py: i32,
  w: i32,
  h: i32,
  dx: f64,
  dy: f64,
) -> f64 {
  let nx = px as f64 / w.max(1) as f64;
  let ny = py as f64 / h.max(1) as f64;
  let t = nx * dx + ny * dy;
  // Normalize to 0..1 range.
  ((t + 1.0) / 2.0).clamp(0.0, 1.0)
}

/// Returns true if the pixel at (`px`, `py`) falls inside a rounded
/// rectangle defined by (`left`, `top`) to (`right`, `bottom`) with
/// the given corner `radius`.
fn is_inside_rounded_rect(
  px: i32,
  py: i32,
  left: i32,
  top: i32,
  right: i32,
  bottom: i32,
  radius: f64,
) -> bool {
  if px < left || px >= right || py < top || py >= bottom {
    return false;
  }

  let r = radius;
  let ri = r as i32;

  // Only corners need the distance check.
  let in_corner = (px < left + ri && py < top + ri)
    || (px >= right - ri && py < top + ri)
    || (px < left + ri && py >= bottom - ri)
    || (px >= right - ri && py >= bottom - ri);

  if !in_corner {
    return true;
  }

  // Find the center of the relevant corner arc.
  let cx = if px < left + ri {
    left as f64 + r
  } else {
    right as f64 - r
  };
  let cy = if py < top + ri {
    top as f64 + r
  } else {
    bottom as f64 - r
  };

  let dx = px as f64 - cx;
  let dy = py as f64 - cy;
  dx * dx + dy * dy <= r * r
}

/// Samples a multi-stop gradient at parameter `t` (0.0..=1.0).
fn sample_gradient(colors: &[Color], t: f64) -> Color {
  if colors.len() <= 1 {
    return colors.first().cloned().unwrap_or(Color {
      r: 255,
      g: 255,
      b: 255,
      a: 255,
    });
  }
  let segments = colors.len() - 1;
  let scaled = t * segments as f64;
  let idx = (scaled.floor() as usize).min(segments - 1);
  let local_t = scaled - idx as f64;
  colors[idx].lerp(&colors[idx + 1], local_t)
}
