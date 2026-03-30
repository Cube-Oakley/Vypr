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
#[derive(Debug, Clone, Copy)]
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

  /// Which edge is being resized (for resize operations).
  pub resize_edge: ResizeEdge,

  /// Tiling direction of the parent (for resize operations).
  pub resize_direction: TilingDirection,
}

/// Handles a mouse event during an alt-drag operation. Returns `true`
/// if the event was consumed.
pub fn handle_alt_drag(
  event: &MouseEvent,
  state: &mut WmState,
  config: &mut UserConfig,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  match event {
    MouseEvent::ButtonDown {
      position, button, ..
    } => {
      if alt_drag.is_some() || !state.dispatcher.is_alt_down() {
        return Ok(false);
      }
      handle_drag_start(position, button, state, config, alt_drag)
    }
    MouseEvent::Move { position, .. } => {
      let Some(drag) = alt_drag.as_mut() else {
        return Ok(false);
      };

      if !state.dispatcher.is_alt_down() {
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

      // Update last cursor for next delta.
      drag.last_cursor = position.clone();
      Ok(true)
    }
    MouseEvent::ButtonUp { button, .. } => {
      let Some(drag) = alt_drag.as_ref() else {
        return Ok(false);
      };

      let matches = matches!(
        (&drag.operation, button),
        (ActiveDragOperation::Move, MouseButton::Left)
          | (ActiveDragOperation::Resize, MouseButton::Right)
      );

      if matches {
        handle_drag_end(state, config, alt_drag)
      } else {
        Ok(false)
      }
    }
  }
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
    resize_direction: TilingDirection::Horizontal,
  });

  Ok(true)
}

/// Starts a resize drag — determines which edge to resize based on
/// cursor position relative to the window center.
fn start_resize_drag(
  position: &Point,
  window: &WindowContainer,
  alt_drag: &mut Option<AltDragState>,
) -> anyhow::Result<bool> {
  let tiling = match window.as_tiling_container() {
    Ok(t) => t,
    Err(_) => return Ok(false),
  };

  if tiling.tiling_siblings().count() == 0 {
    return Ok(false);
  }

  let parent = window
    .direction_container()
    .context("No direction container.")?;

  let direction = parent.tiling_direction();
  let rect = window.to_rect()?;

  // Determine which edge based on cursor position relative to window
  // center along the parent's tiling axis.
  let resize_edge = match direction {
    TilingDirection::Horizontal => {
      if position.x < rect.center_point().x {
        ResizeEdge::Start
      } else {
        ResizeEdge::End
      }
    }
    TilingDirection::Vertical => {
      if position.y < rect.center_point().y {
        ResizeEdge::Start
      } else {
        ResizeEdge::End
      }
    }
  };

  *alt_drag = Some(AltDragState {
    window_id: window.id(),
    operation: ActiveDragOperation::Resize,
    is_from_floating: false,
    start_cursor: position.clone(),
    last_cursor: position.clone(),
    resize_edge,
    resize_direction: direction,
  });

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

/// Adjusts the tiling split ratio based on cursor movement delta.
/// Only the dragged window and its adjacent neighbor on the dragged
/// edge are affected — other siblings remain untouched. This creates
/// the Hyprland-style edge resize behavior.
fn handle_resize_drag(
  position: &Point,
  drag: &AltDragState,
  window: &WindowContainer,
  state: &mut WmState,
) -> anyhow::Result<()> {
  use crate::traits::MIN_TILING_SIZE;

  let tiling = window
    .as_tiling_container()
    .context("Not a tiling container.")?;

  let parent = window
    .direction_container()
    .context("No direction container.")?;

  let parent_rect = parent.to_rect()?;
  let parent_size = match drag.resize_direction {
    TilingDirection::Horizontal => parent_rect.width(),
    TilingDirection::Vertical => parent_rect.height(),
  };

  if parent_size == 0 {
    return Ok(());
  }

  // Find the adjacent sibling on the dragged edge.
  let window_index = window.index();
  let siblings: Vec<_> = parent.tiling_children().collect();

  let neighbor = match drag.resize_edge {
    // Dragging the right/bottom edge → neighbor is the next sibling.
    ResizeEdge::End => siblings
      .iter()
      .find(|s| s.index() == window_index + 1),
    // Dragging the left/top edge → neighbor is the previous sibling.
    ResizeEdge::Start => {
      if window_index > 0 {
        siblings
          .iter()
          .find(|s| s.index() == window_index - 1)
      } else {
        None
      }
    }
  };

  let Some(neighbor) = neighbor else {
    return Ok(());
  };

  // Calculate pixel delta since last move event.
  let delta_px = match drag.resize_direction {
    TilingDirection::Horizontal => position.x - drag.last_cursor.x,
    TilingDirection::Vertical => position.y - drag.last_cursor.y,
  };

  let size_delta = delta_px as f32 / parent_size as f32;

  // For the start edge, invert the delta direction.
  let adjusted_delta = match drag.resize_edge {
    ResizeEdge::Start => -size_delta,
    ResizeEdge::End => size_delta,
  };

  // Transfer size between the two containers only.
  let new_window_size =
    (tiling.tiling_size() + adjusted_delta).max(MIN_TILING_SIZE);
  let new_neighbor_size =
    (neighbor.tiling_size() - adjusted_delta).max(MIN_TILING_SIZE);

  tiling.set_tiling_size(new_window_size);
  neighbor.set_tiling_size(new_neighbor_size);

  // Queue parent for redraw.
  let parent_container: Container = parent.into();
  state
    .pending_sync
    .queue_container_to_redraw(parent_container);

  Ok(())
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
      drop_window_at_cursor(window, state, config)?;
    }
    ActiveDragOperation::Resize => {
      // Resize is already applied incrementally. Final redraw.
      if let Some(parent) = window.parent() {
        state.pending_sync.queue_container_to_redraw(parent);
      }
    }
  }

  Ok(true)
}

/// Drops a floating window back into the tiling tree at the cursor
/// position. The window is detached, converted to tiling, and
/// inserted directly at the drop target — all in one step to avoid
/// intermediate tree mutations that shift indices.
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

  // Get the non-tiling window so we can convert it.
  let non_tiling = window
    .as_non_tiling_window()
    .context("Expected non-tiling window.")?
    .clone();

  // Find drop target while the tree is still stable (window is
  // floating and not in the tiling tree's layout).
  let containers_at_pos = state
    .containers_at_point(&workspace.clone().into(), &mouse_pos)
    .into_iter()
    .filter(|c| c.id() != window.id());

  let target_parent = containers_at_pos
    .filter_map(|c| c.as_direction_container().ok())
    .fold(
      DirectionContainer::from(workspace.clone()),
      |acc, c| {
        if c.ancestors().count() > acc.ancestors().count() {
          c
        } else {
          acc
        }
      },
    );

  // Find nearest and compute drop action.
  enum DropAction {
    Insert { index: usize },
    Split {
      target_id: uuid::Uuid,
      split_direction: TilingDirection,
      index: usize,
    },
    Empty,
  }

  let drop_action = if target_parent.tiling_children().count() > 0 {
    let nearest = target_parent
      .tiling_children()
      .try_fold(None, |acc, c| {
        let dist = c.to_rect()?.distance_to_point(&mouse_pos);
        match acc {
          Some((_, best_dist)) if dist >= best_dist => {
            anyhow::Ok(Some((c, best_dist)))
          }
          _ => Ok(Some((c, dist))),
        }
      })?
      .map(|(c, _)| c)
      .context("No nearest container.")?;

    let nearest_rect = nearest.to_rect()?;
    let direction = target_parent.tiling_direction();

    let dx = mouse_pos.x - nearest_rect.center_point().x;
    let dy = mouse_pos.y - nearest_rect.center_point().y;

    if dx.abs() > dy.abs() {
      let is_left = dx < 0;
      if direction == TilingDirection::Horizontal {
        DropAction::Insert {
          index: if is_left {
            nearest.index()
          } else {
            nearest.index() + 1
          },
        }
      } else {
        DropAction::Split {
          target_id: nearest.id(),
          split_direction: TilingDirection::Horizontal,
          index: if is_left { 0 } else { 1 },
        }
      }
    } else {
      let is_top = dy < 0;
      if direction == TilingDirection::Vertical {
        DropAction::Insert {
          index: if is_top {
            nearest.index()
          } else {
            nearest.index() + 1
          },
        }
      } else {
        DropAction::Split {
          target_id: nearest.id(),
          split_direction: TilingDirection::Vertical,
          index: if is_top { 0 } else { 1 },
        }
      }
    }
  } else {
    DropAction::Empty
  };

  // --- Atomic conversion: detach floating → create tiling → insert ---

  // Detach the floating window from the tree.
  let ancestors = non_tiling.ancestors().take(3).collect::<Vec<_>>();
  detach_container(non_tiling.clone().into())?;
  for ancestor in ancestors.iter().rev() {
    flatten_child_split_containers(ancestor)?;
  }

  // Convert to tiling window.
  let tiling_window =
    non_tiling.to_tiling(config.value.gaps.clone());

  // Insert at the drop target.
  match drop_action {
    DropAction::Empty => {
      attach_container(
        &tiling_window.clone().into(),
        &target_parent.clone().into(),
        Some(0),
      )?;
    }
    DropAction::Insert { index } => {
      let max_idx = target_parent.tiling_children().count();
      attach_container(
        &tiling_window.clone().into(),
        &target_parent.clone().into(),
        Some(index.min(max_idx)),
      )?;
    }
    DropAction::Split {
      target_id,
      split_direction,
      index,
    } => {
      let target = target_parent
        .tiling_children()
        .find(|c| c.id() == target_id);

      if let Some(target) = target {
        let split = SplitContainer::new(
          split_direction,
          config.value.gaps.clone(),
        );

        wrap_in_split_container(
          &split,
          &target_parent.clone().into(),
          &[target],
        )?;

        attach_container(
          &tiling_window.clone().into(),
          &split.into(),
          Some(index),
        )?;
      } else {
        attach_container(
          &tiling_window.clone().into(),
          &target_parent.clone().into(),
          Some(target_parent.tiling_children().count()),
        )?;
      }
    }
  }

  // Redraw everything in the workspace.
  state
    .pending_sync
    .queue_containers_to_redraw(workspace.tiling_children())
    .queue_focus_change()
    .queue_workspace_to_reorder(workspace);

  Ok(())
}
