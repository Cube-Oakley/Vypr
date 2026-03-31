<div align="center">
  <br>
  <img src="./resources/assets/logo.svg" width="230" alt="Vypr logo" />
  <br>

# Vypr

**A tiling window manager for Windows, forked from [GlazeWM](https://github.com/glzr-io/glazewm).**

Vypr builds on GlazeWM's solid foundation with new features focused on a smoother, more polished desktop experience — including alt-drag window management, acrylic blur effects, gradient borders, dwindle auto-tiling, and more.

![Demo video][demo-video]

</div>

## What's new in Vypr

Vypr extends GlazeWM with the following features, all built from scratch:

- **Dwindle auto-tiling** — Automatically tiles windows in a dwindle (spiral) layout, similar to Hyprland.
- **Alt+drag move & resize** — Move windows with `Alt+LMB` and resize with `Alt+RMB`, inspired by Linux window managers. Includes edge snapping and proper floating/tiling integration.
- **Acrylic blur & transparency** — Apply Windows acrylic blur and adjustable transparency to windows for a modern, polished look.
- **Gradient border overlays** — Multi-color gradient borders drawn as lightweight overlay windows, replacing the flat single-color borders from GlazeWM.
- **Taskbar auto-hide** — Automatically hides the Windows taskbar and blocks the Windows key from pulling it back up, keeping your workspace clean.
- **Performance optimizations** — Batched window positioning via `DeferWindowPos`, elimination of low-level mouse hooks that caused input lag, and smoother resize handling.

## Where we're heading

Vypr's goal is to be a fast, opinionated tiling WM for Windows that feels as natural as the Linux alternatives. Planned areas of focus include:

- More automatic tiling layouts (master-stack, grid, etc.)
- Richer window effects and animations
- Better multi-monitor ergonomics
- First-class support for ultra-wide and mixed-DPI setups

## Installation

Vypr is not yet available through package managers. To build from source:

```sh
git clone https://github.com/Cube-Oakley/Vypr.git
cd Vypr
cargo build --release
```

The built binary will be at `target/release/vypr.exe`.

## Default keybindings

On first launch, a default configuration can optionally be generated.

Below is a cheat sheet of all available commands and their default keybindings.

![Infographic](/resources/assets/cheatsheet.png)

## Configuration

The default config file is generated at `%userprofile%\.vypr\config.yaml`.

To use a different config file location, launch with the `--config` flag:

```sh
vypr.exe start --config="C:\<PATH_TO_CONFIG>\config.yaml"
```

Or set the `VYPR_CONFIG_PATH` environment variable:

```sh
setx VYPR_CONFIG_PATH "C:\<PATH_TO_CONFIG>\config.yaml"
```

### General

```yaml
general:
  # Commands to run when the WM has started (e.g. to run a script or launch
  # another application).
  startup_commands: []

  # Commands to run just before the WM is shutdown.
  shutdown_commands: []

  # Commands to run after the WM config has reloaded.
  config_reload_commands: []

  # Whether to automatically focus windows underneath the cursor.
  focus_follows_cursor: false

  # Whether to switch back and forth between the previously focused
  # workspace when focusing the current workspace.
  toggle_workspace_on_refocus: false

  cursor_jump:
    # Whether to automatically move the cursor on the specified trigger.
    enabled: true

    # Trigger for cursor jump:
    # - 'monitor_focus': Jump when focus changes between monitors.
    # - 'window_focus': Jump when focus changes between windows.
    trigger: "monitor_focus"
```

### Keybindings

The available keyboard shortcuts can be customized via the `keybindings` option. A keybinding consists of one or more key combinations and one or more commands to run when pressed.

It's recommended to use the alt key for keybindings. The Windows key is unfortunately a pain to remap, since the OS reserves certain keybindings (e.g. `lwin+l`).

```yaml
keybindings:
  - commands: ["focus --workspace 1"]
    bindings: ["alt+1"]

  # Multiple commands can be run in a sequence.
  - commands: ["move --workspace 1", "focus --workspace 1"]
    bindings: ["alt+shift+1"]
```

<details>
<summary>Full list of supported keys</summary>

| Key                   | Description                                                               |
| --------------------- | ------------------------------------------------------------------------- |
| `a` - `z`             | Alphabetical letter keys                                                  |
| `0` - `9`             | Number keys                                                               |
| `numpad0` - `numpad9` | Numerical keypad keys                                                     |
| `f1` - `f24`          | Function keys                                                             |
| `shift`               | Either left or right SHIFT key                                            |
| `lshift`              | The left SHIFT key                                                        |
| `rshift`              | The right SHIFT key                                                       |
| `control`             | Either left or right CTRL key                                             |
| `lctrl`               | The left CTRL key                                                         |
| `rctrl`               | The right CTRL key                                                        |
| `alt`                 | Either left or right ALT key                                              |
| `lalt`                | The left ALT key                                                          |
| `ralt`                | The right ALT key                                                         |
| `lwin`                | The left Windows logo key                                                 |
| `rwin`                | The right Windows logo key                                                |
| `space`               | The spacebar key                                                          |
| `escape`              | The ESCAPE key                                                            |
| `back`                | The BACKSPACE key                                                         |
| `tab`                 | The TAB key                                                               |
| `enter`               | The ENTER key                                                             |
| `left`                | The left arrow key                                                        |
| `right`               | The right arrow key                                                       |
| `up`                  | The up arrow key                                                          |
| `down`                | The down arrow key                                                        |
| `num_lock`            | The NUM LOCK key                                                          |
| `scroll_lock`         | The SCROLL LOCK key                                                       |
| `caps_lock`           | The CAPS LOCK key                                                         |
| `page_up`             | The PAGE UP key                                                           |
| `page_down`           | The PAGE DOWN key                                                         |
| `insert`              | The INSERT key                                                            |
| `delete`              | The DELETE key                                                            |
| `end`                 | The END key                                                               |
| `home`                | The HOME key                                                              |
| `print_screen`        | The PRINT SCREEN key                                                      |
| `multiply`            | The `*` key (numpad only)                                                 |
| `add`                 | The `+` key (numpad only)                                                 |
| `subtract`            | The `-` key (numpad only)                                                 |
| `decimal`             | The DEL key (numpad only)                                                 |
| `divide`              | The `/` key (numpad only)                                                 |
| `volume_up`           | The volume up key                                                         |
| `volume_down`         | The volume down key                                                       |
| `volume_mute`         | The volume mute key                                                       |
| `media_next_track`    | The media next track key                                                  |
| `media_prev_track`    | The media prev track key                                                  |
| `media_stop`          | The media stop key                                                        |
| `media_play_pause`    | The media play/pause key                                                  |
| `oem_semicolon`       | The `;`/`:` key (US layout)                                              |
| `oem_question`        | The `/`/`?` key (US layout)                                              |
| `oem_tilde`           | The `` ` ``/`~` key (US layout)                                          |
| `oem_open_brackets`   | The `[`/`{` key (US layout)                                              |
| `oem_pipe`            | The `\`/`\|` key (US layout)                                             |
| `oem_close_brackets`  | The `]`/`}` key (US layout)                                              |
| `oem_quotes`          | The `'`/`"` key (US layout)                                              |
| `oem_plus`            | The `=`/`+` key (US layout)                                              |
| `oem_comma`           | The `,`/`<` key (US layout)                                              |
| `oem_minus`           | The `-`/`_` key (US layout)                                              |
| `oem_period`          | The `.`/`>` key (US layout)                                              |

</details>

If a key is not listed above, it's likely still supported if you use its character directly (e.g. `alt+å`).

> German and US international keyboards treat the right-side alt key differently. For these layouts, use `ralt+ctrl` instead of `ralt`.

### Gaps

```yaml
gaps:
  # Gap between adjacent windows.
  inner_gap: "20px"

  # Gap between windows and the screen edge.
  outer_gap:
    top: "20px"
    right: "20px"
    bottom: "20px"
    left: "20px"
```

### Workspaces

```yaml
workspaces:
  - name: "1"
    display_name: "Work"
    bind_to_monitor: 0
    keep_alive: false
```

### Window rules

Commands can be run when a window is first launched — useful for always floating certain apps or assigning them to specific workspaces.

```yaml
window_rules:
  - commands: ["move --workspace 1"]
    match:
      - window_process: { regex: "msedge|brave|chrome" }

  - commands: ["ignore"]
    match:
      - window_process: { equals: "zebar" }

      - window_title: { regex: "[Pp]icture.in.[Pp]icture" }
        window_class: { regex: "Chrome_WidgetWin_1|MozillaDialogClass" }
```

### Window effects

```yaml
window_effects:
  focused_window:
    border:
      enabled: true
      color: "#0000ff"

  other_windows:
    border:
      enabled: false
      color: "#d3d3d3"
```

### Window behavior

```yaml
window_behavior:
  # Allowed values: 'tiling', 'floating'.
  initial_state: "tiling"

  state_defaults:
    floating:
      centered: true
      shown_on_top: false

    fullscreen:
      maximized: false
```

### Binding modes

Binding modes modify keybindings while Vypr is running. Enable with `wm-enable-binding-mode --name <NAME>`, disable with `wm-disable-binding-mode --name <NAME>`.

```yaml
binding_modes:
  - name: "resize"
    keybindings:
      - commands: ["resize --width -2%"]
        bindings: ["h", "left"]
      - commands: ["resize --width +2%"]
        bindings: ["l", "right"]
      - commands: ["resize --height +2%"]
        bindings: ["k", "up"]
      - commands: ["resize --height -2%"]
        bindings: ["j", "down"]
      - commands: ["wm-disable-binding-mode --name resize"]
        bindings: ["escape", "enter"]
```

## FAQ

**How do I run Vypr on startup?**

Right-click the Vypr icon in the system tray and select "Run on system startup".

**How can I create a custom layout?**

Change the tiling direction with `alt+v`. This controls where the next window is placed relative to the current one — horizontal places it to the right, vertical places it below. Vypr also supports automatic dwindle tiling out of the box.

**How do I create a rule for a specific app?**

You'll need the window's process name, title, or class name. Tools like Winlister or AutoHotkey's Window Spy can help. Example:

```yaml
window_rules:
  - commands: ["set-floating"]
    match:
      - window_process: { equals: "Flow.Launcher" }
        window_title: { equals: "Settings" }
```

**Can I disable keybindings for a specific app?**

Not yet. The default keybinding `alt+shift+p` toggles all keybindings on/off as a workaround.

## Acknowledgements

Vypr is a fork of [GlazeWM](https://github.com/glzr-io/glazewm) by [glzr-io](https://github.com/glzr-io). Huge thanks to the GlazeWM team for building the foundation that makes this project possible.

## License

Vypr is licensed under the [GPLv3 license](LICENSE.md).

[demo-video]: resources/assets/demo.webp
