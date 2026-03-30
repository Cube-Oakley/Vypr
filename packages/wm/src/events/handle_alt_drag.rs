use anyhow::Context;
use wm_common::{
  ActiveDrag, ActiveDragOperation, TilingDirection, WindowState,
};
use wm_platform::{MouseButton, MouseEvent, Point};

use crate::{
  commands::{
    container::{
      attach_container, detach_container,
      flatten_child_split_containers, move_container_within_tree,
      wrap_in_split_container,
    },
    window::update_window_state,
  },
  models::{Container, DirectionContainer, SplitContainer, WindowContainer},
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters,
    TilingSizeGetters, WindowGetters,
  },
  user_config::UserConfig,
  wm_state::WmState,
};

/// Which edge of the window the resize drag is anchored to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ResizeEdge {
  /// Dragging resizes the left/top boundary.
  Start,
  /// Dragging resizes the right/bottom boundary.
  End,
}

/// State tracked during an active alt-drag operation.
pub struct AltDragState {
  /// The window being dragged.
  pub window_id: uuid::Uuid,

  /// The operation type (move or resize).
  pub operation: ActiveDragOperation,

  /// Whether the window was floating before the drag started.
  pub is_from_floating: bool,

  /// Cursor position when the drag started.
  pub start_cursor: Point,

  /// Last cursor position (for delta calculation).
  pub last_cursor: Point,

  /// Which horizontal edge is being resized (left/right).
  pub resize_edge: ResizeEdge,

  /// Which vertical edge is being resized (top/bottom).
  pub resize_edge_v: ResizeEdge,
}

/// Handles a mouse event during an alt-drag operation. Returns `true`
/// if the event was consumed.
pub fn handle_alt_drag(
  event: &MouseEvent,
  state: &mut WmState,
  config: &mut UserConfig,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  // All alt-drag detection happens during Move events, using
  // GetAsyncKeyState to check button state. This avoids depending
  // on raw input ButtonDown/ButtonUp events which are unreliable
  // via RIDEV_INPUTSINK.
  let MouseEvent::Move {
    position,
    pressed_buttons,
    ..
  } = event
  else {
    // Still handle ButtonUp for move-drag end detection via raw
    // input (LeftButtonUp is reliable since upstream used it).
    if let MouseEvent::ButtonUp {
      button: MouseButton::Left,
      ..
    } = event
    {
      if alt_drag
        .as_ref()
        .is_some_and(|d| d.operation == ActiveDragOperation::Move)
      {
        return handle_drag_end(state, config, alt_drag);
      }
    }
    return Ok(false);
  };

  let alt_down = state.dispatcher.is_alt_down();
  let left_down =
    state.dispatcher.is_mouse_down(&MouseButton::Left);
  let right_down =
    state.dispatcher.is_mouse_down(&MouseButton::Right);

  // Start a new drag if Alt + button is held and no drag active.
  if alt_drag.is_none() && alt_down {
    if left_down {
      return handle_drag_start(
        position,
        &MouseButton::Left,
        state,
        config,
        alt_drag,
      );
    }
    if right_down {
      return handle_drag_start(
        position,
        &MouseButton::Right,
        state,
        config,
        alt_drag,
      );
    }
  }

  let Some(drag) = alt_drag.as_mut() else {
    return Ok(false);
  };

  // End drag if the button or Alt was released.
  let should_end = match drag.operation {
    ActiveDragOperation::Move => !left_down || !alt_down,
    ActiveDragOperation::Resize => !right_down || !alt_down,
  };

  if should_end {
    return handle_drag_end(state, config, alt_drag);
  }

  let window = state
    .windows()
    .into_iter()
    .find(|w| w.id() == drag.window_id);

  let Some(window) = window else {
    *alt_drag = None;
    return Ok(false);
  };

  match drag.operation {
    ActiveDragOperation::Move => {
      handle_move_drag(position, &window)?;
    }
    ActiveDragOperation::Resize => {
      handle_resize_drag(position, drag, &window, state)?;
    }
  }

  drag.last_cursor = position.clone();
  Ok(true)
}

/// Initiates an alt-drag operation.
fn handle_drag_start(
  position: &Point,
  button: &MouseButton,
  state: &mut WmState,
  config: &mut UserConfig,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  let native = state.dispatcher.window_from_point(position)?;
  let Some(native) = native else {
    return Ok(false);
  };
  let window = state.window_from_native(&native);
  let Some(window) = window else {
    return Ok(false);
  };

  match button {
    MouseButton::Left => {
      start_move_drag(position, &window, state, config, alt_drag)
    }
    MouseButton::Right => {
      start_resize_drag(position, &window, alt_drag)
    }
  }
}

/// Starts a move drag — floats the window and tracks cursor.
fn start_move_drag(
  position: &Point,
  window: &WindowContainer,
  state: &mut WmState,
  config: &mut UserConfig,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  let window_id = window.id();
  let is_from_floating = !matches!(window.state(), WindowState::Tiling);
  let frame = window.native().frame().map_err(anyhow::Error::from)?;

  window.set_active_drag(Some(ActiveDrag {
    operation: Some(ActiveDragOperation::Move),
    is_from_floating,
    initial_position: frame,
  }));

  // Float the window if it's tiling, preserving its current size.
  if !is_from_floating {
    // Save the current tiling rect so the floating window keeps the
    // same size during drag instead of expanding.
    let tiling_rect = window.to_rect().ok();

    let floating_config =
      config.value.window_behavior.state_defaults.floating.clone();

    let floated = update_window_state(
      window.clone(),
      WindowState::Floating(floating_config),
      state,
      config,
    )?;

    // Override the floating placement with the tiling rect so the
    // window doesn't jump to a larger size.
    if let Some(rect) = tiling_rect {
      if let Some(non_tiling) = floated.as_non_tiling_window() {
        non_tiling.set_floating_placement(rect);
        non_tiling.set_has_custom_floating_placement(true);
      }
    }
  }

  *alt_drag = Some(AltDragState {
    window_id,
    operation: ActiveDragOperation::Move,
    is_from_floating,
    start_cursor: position.clone(),
    last_cursor: position.clone(),
    resize_edge: ResizeEdge::End,
    resize_edge_v: ResizeEdge::End,
  });

  Ok(true)
}

/// Starts a resize drag — determines which edges to resize based on
/// cursor position relative to the window center.
fn start_resize_drag(
  position: &Point,
  window: &WindowContainer,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  // Only resize tiling windows.
  if window.as_tiling_container().is_err() {
    return Ok(false);
  }

  let rect = window.to_rect()?;

  // Determine edge for each axis based on cursor position relative
  // to window center.
  let h_edge = if position.x < rect.center_point().x {
    ResizeEdge::Start
  } else {
    ResizeEdge::End
  };
  let v_edge = if position.y < rect.center_point().y {
    ResizeEdge::Start
  } else {
    ResizeEdge::End
  };

  *alt_drag = Some(AltDragState {
    window_id: window.id(),
    operation: ActiveDragOperation::Resize,
    is_from_floating: false,
    start_cursor: position.clone(),
    last_cursor: position.clone(),
    resize_edge: h_edge,
    resize_edge_v: v_edge,
  });

  // Dismiss any context menu or right-click action that the initial
  // WM_RBUTTONDOWN caused in the target window. This is a no-hook
  // alternative to WH_MOUSE_LL blocking.
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
      SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT,
      KEYEVENTF_KEYUP, VK_ESCAPE,
    };

    let inputs = [
      INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
          ki: KEYBDINPUT {
            wVk: VK_ESCAPE,
            ..Default::default()
          },
        },
      },
      INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
          ki: KEYBDINPUT {
            wVk: VK_ESCAPE,
            dwFlags: KEYEVENTF_KEYUP,
            ..Default::default()
          },
        },
      },
    ];

    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
  }

  Ok(true)
}

/// Moves the floating window to follow the cursor using raw Win32
/// for minimal latency.
fn handle_move_drag(
  position: &Point,
  window: &WindowContainer,
) -> anyhow::Result<()> {
  let frame = window.native().frame().map_err(anyhow::Error::from)?;

  let new_x = position.x - frame.width() / 2;
  let new_y = position.y - frame.height() / 2;

  window.native().reposition(new_x, new_y)?;
  Ok(())
}

/// Adjusts tiling split ratios based on cursor movement delta.
/// Handles both horizontal and vertical axes simultaneously by
/// walking up the container tree to find the appropriate parent for
/// each direction.
fn handle_resize_drag(
  position: &Point,
  drag: &AltDragState,
  window: &WindowContainer,
  state: &mut WmState,
) -> anyhow::Result<()> {
  use crate::traits::MIN_TILING_SIZE;

  let delta_x = position.x - drag.last_cursor.x;
  let delta_y = position.y - drag.last_cursor.y;

  // Try to resize in each axis by finding the nearest ancestor
  // split that uses that direction.
  let axes: [(i32, TilingDirection, &ResizeEdge); 2] = [
    (delta_x, TilingDirection::Horizontal, &drag.resize_edge),
    (delta_y, TilingDirection::Vertical, &drag.resize_edge_v),
  ];

  for (delta_px, direction, edge) in &axes {
    if *delta_px == 0 {
      continue;
    }

    // Walk up the tree to find an ancestor that uses this tiling
    // direction. The resized container is the ancestor's child that
    // contains our window.
    let result = find_resizable_ancestor(window, &direction, edge);
    let Some((container_to_resize, neighbor)) = result else {
      continue;
    };

    let parent_rect = container_to_resize
      .parent()
      .and_then(|p| p.to_rect().ok());
    let Some(parent_rect) = parent_rect else {
      continue;
    };

    let parent_size = match *direction {
      TilingDirection::Horizontal => parent_rect.width(),
      TilingDirection::Vertical => parent_rect.height(),
    };

    if parent_size == 0 {
      continue;
    }

    let size_delta = *delta_px as f32 / parent_size as f32;
    let adjusted_delta = match edge {
      ResizeEdge::Start => -size_delta,
      ResizeEdge::End => size_delta,
    };

    let new_size =
      (container_to_resize.tiling_size() + adjusted_delta)
        .max(MIN_TILING_SIZE);
    let new_neighbor_size =
      (neighbor.tiling_size() - adjusted_delta).max(MIN_TILING_SIZE);

    container_to_resize.set_tiling_size(new_size);
    neighbor.set_tiling_size(new_neighbor_size);

    if let Some(parent) = container_to_resize.parent() {
      state
        .pending_sync
        .queue_container_to_redraw(parent);
    }
  }

  Ok(())
}

/// Walks up the tree from `window` to find an ancestor whose parent
/// uses the given tiling direction AND has an adjacent sibling.
/// Returns `(container_to_resize, neighbor)`.
fn find_resizable_ancestor(
  window: &WindowContainer,
  direction: &TilingDirection,
  edge: &ResizeEdge,
) -> Option<(
  crate::models::TilingContainer,
  crate::models::TilingContainer,
)> {
  let mut current: crate::models::Container = window.clone().into();

  for _ in 0..20 {
    let parent = current.parent()?;
    let dir_container = parent.as_direction_container().ok()?;

    if dir_container.tiling_direction() == *direction {
      let tiling = current.as_tiling_container().ok()?;
      let idx = current.index();
      let siblings: Vec<_> = dir_container.tiling_children().collect();

      // Pick the neighbor on the correct side based on the drag
      // edge. Start (left/top) looks for the previous sibling,
      // End (right/bottom) looks for the next sibling.
      let neighbor = match edge {
        ResizeEdge::Start => {
          if idx > 0 {
            siblings.iter().find(|s| s.index() == idx - 1)
          } else {
            None
          }
        }
        ResizeEdge::End => {
          siblings.iter().find(|s| s.index() == idx + 1)
        }
      };

      if let Some(n) = neighbor {
        return Some((tiling, n.clone()));
      }

      // No neighbor on this edge at this level — keep walking up.
    }

    current = parent;
  }

  None
}

/// Ends the alt-drag operation and retiles the window at the cursor
/// position if it was a move drag.
fn handle_drag_end(
  state: &mut WmState,
  config: &mut UserConfig,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  let drag = alt_drag.take().context("No active drag.")?;

  let window = state
    .windows()
    .into_iter()
    .find(|w| w.id() == drag.window_id);

  let Some(window) = window else {
    return Ok(true);
  };

  match drag.operation {
    ActiveDragOperation::Move => {
      window.set_active_drag(None);

      if drag.is_from_floating {
        let container: Container = window.into();
        state.pending_sync.queue_container_to_redraw(container);
        return Ok(true);
      }

          // Clear the insertion target so update_window_state doesn't
      // snap the window back to its original position.
      if let Some(non_tiling) = window.as_non_tiling_window() {
        non_tiling.set_insertion_target(None);
      }

      // Find the best drop target at the current cursor position.
      // Errors here should not crash the WM — log and continue.
      if let Err(err) = drop_window_at_cursor(window, state, config)
      {
        tracing::warn!("Failed to drop window at cursor: {err}");
      }
    }
    ActiveDragOperation::Resize => {
      // Resize is already applied incrementally. Final redraw.
      if let Some(parent) = window.parent() {
        state.pending_sync.queue_container_to_redraw(parent);
      }
    }
  }

  // Re-focus the window under the cursor so borders and focus
  // update correctly after the drag.
  if let Ok(cursor_pos) = state.dispatcher.cursor_position() {
    if let Ok(Some(native)) =
      state.dispatcher.window_from_point(&cursor_pos)
    {
      if let Some(w) = state.window_from_native(&native) {
        use crate::commands::container::set_focused_descendant;
        set_focused_descendant(&w.into(), None);
      }
    }
  }

  state.pending_sync.queue_focus_change();
  state.pending_sync.queue_all_effects_update();

  // Run a full platform_sync immediately so updates are visible
  // even if the mouse doesn't move after release.
  crate::commands::general::platform_sync(state, config)?;

  Ok(true)
}

/// Drops a floating window back into the tiling tree at the cursor
/// position. Finds the nearest tiling WINDOW (leaf node) across the
/// entire workspace, then inserts relative to it based on quadrant
/// analysis.
fn drop_window_at_cursor(
  window: WindowContainer,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let mouse_pos = state.dispatcher.cursor_position()?;

  let workspace = state
    .monitor_at_point(&mouse_pos)
    .and_then(|m| m.displayed_workspace())
    .or_else(|| window.workspace())
    .context("No workspace for drop.")?;

  let non_tiling = window
    .as_non_tiling_window()
    .context("Expected non-tiling window.")?
    .clone();

  // Find the nearest tiling WINDOW across the entire workspace
  // (not just direct children of one container). This handles
  // gaps, edges, and deeply nested layouts.
  let all_tiling_windows: Vec<_> = workspace
    .descendants()
    .filter(|c| c.id() != non_tiling.id())
    .filter_map(|c| c.as_tiling_container().ok())
    .filter(|c| c.is_tiling_window())
    .collect();

  // --- Atomic conversion: detach floating → create tiling → insert ---

  let ancestors = non_tiling.ancestors().take(3).collect::<Vec<_>>();
  detach_container(non_tiling.clone().into())?;
  for ancestor in ancestors.iter().rev() {
    flatten_child_split_containers(ancestor)?;
  }

  let tiling_window =
    non_tiling.to_tiling(config.value.gaps.clone());

  if all_tiling_windows.is_empty() {
    // Empty workspace — just insert.
    attach_container(
      &tiling_window.clone().into(),
      &workspace.clone().into(),
      Some(0),
    )?;
  } else {
    // Find nearest window by distance to cursor.
    let nearest = all_tiling_windows
      .iter()
      .filter_map(|c| c.to_rect().ok().map(|r| (c, r)))
      .min_by(|(_, a), (_, b)| {
        a.distance_to_point(&mouse_pos)
          .partial_cmp(&b.distance_to_point(&mouse_pos))
          .unwrap_or(std::cmp::Ordering::Equal)
      })
      .map(|(c, _)| c)
      .context("No nearest window.")?;

    let nearest_rect = nearest.to_rect()?;
    let parent = nearest
      .parent()
      .context("Nearest has no parent.")?;
    let direction = nearest
      .direction_container()
      .context("No direction container.")?
      .tiling_direction();

    // Quadrant analysis relative to the nearest window.
    let dx = mouse_pos.x - nearest_rect.center_point().x;
    let dy = mouse_pos.y - nearest_rect.center_point().y;

    let (is_before, needs_split) = if dx.abs() > dy.abs() {
      let is_left = dx < 0;
      (is_left, direction == TilingDirection::Vertical)
    } else {
      let is_top = dy < 0;
      (is_top, direction == TilingDirection::Horizontal)
    };

    if needs_split {
      // Create a perpendicular split.
      let split_dir = if dx.abs() > dy.abs() {
        TilingDirection::Horizontal
      } else {
        TilingDirection::Vertical
      };

      let split = SplitContainer::new(
        split_dir,
        config.value.gaps.clone(),
      );

      wrap_in_split_container(
        &split,
        &parent,
        &[nearest.clone()],
      )?;

      attach_container(
        &tiling_window.clone().into(),
        &split.into(),
        Some(if is_before { 0 } else { 1 }),
      )?;
    } else {
      // Insert as sibling.
      let idx = if is_before {
        nearest.index()
      } else {
        nearest.index() + 1
      };
      let max_idx = parent
        .as_direction_container()
        .map(|d| d.tiling_children().count())
        .unwrap_or(0);

      attach_container(
        &tiling_window.clone().into(),
        &parent,
        Some(idx.min(max_idx)),
      )?;
    }
  }

  state
    .pending_sync
    .queue_containers_to_redraw(workspace.tiling_children())
    .queue_focus_change()
    .queue_workspace_to_reorder(workspace);

  Ok(())
}
