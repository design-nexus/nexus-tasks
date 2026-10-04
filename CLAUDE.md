# Tasks — notes for working on this repo

- GTK4 (gtk4-rs 0.11) + Rust. No libadwaita. The window is one flat, monospace surface
  split by hairlines: a top bar (sidebar toggle, `Tasks / <page>`, the page's config
  file, search, settings, close), the sidebar, the page and a status bar (`F1 Shortcuts`,
  processes/CPU/memory, the live/paused pill). Pages have no title header. Settings
  is a card over the window (`settings_dialog.rs`) listing the settings page's groups;
  `navigate("settings")` opens it. Ctrl+F filters processes, or opens Go to elsewhere.
  Theme, window, widgets and stylesheet started as copies of Settings (`~/Projects/settings`).
  Every colour is a `@theme_*` token; graphs get theirs from `theme::palette()`.
- `sampler/` runs on its own thread and never touches GTK. It sends a `Snapshot` per
  interval over an async channel. `live.rs` keeps graph history (ring buffers by key,
  e.g. `cpu`, `disk.nvme0n1.read`, `gpu.<pci>.util`) and calls `live::on_tick`
  subscribers only while their widget is mapped. Pages whose layout depends on the
  hardware build inside `live::ready`.
- Processes: one `ProcObject` per pid in a `ListStore`. `layout()` computes order,
  depth, visibility and collapsed totals; the GTK sorter/filter just follow `order` and
  `visible`. GTK re-binds rows synchronously, so never hold a `STATE` borrow across
  store/filter/sorter changes. `apply_view` also keeps the scroll position still
  (ListView otherwise follows its anchor row after a re-sort).
- Don't wake the dGPU: no `lspci`, no Vulkan renderer (main.rs sets `GSK_RENDERER=ngl`),
  `nvidia-smi` only while `runtime_status` isn't `suspended`. Check with
  `cat /sys/bus/pci/devices/0000:01:00.0/power/runtime_status` while the app runs.
- Never edit the user's Hyprland files except the one `require` line added on request
  (`startup::add_include`, with a backup). Managed items are generated into `hypr/tasks.lua`.
- Hyprland dispatch: `hyprctl eval 'hl.dispatch(hl.dsp.focus({ window = "address:0x…" }))'`
  (and `hl.dsp.window.close`).
- Checks: `cargo clippy -- -D warnings`, `cargo test`. Visual check:
  `TASKS_SNAPSHOT=/tmp/x.png [TASKS_SNAPSHOT_PAGE=1] tasks --section cpu`
  (quit any running instance first; it's single-instance). A scratch
  `XDG_CONFIG_HOME` with `theme = "catppuccin-latte"`, `mode = "theme"` checks light mode.
