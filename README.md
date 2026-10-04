# Tasks

A task manager for [Omarchy](https://omarchy.org). It takes its colours from your
Omarchy theme and fits a half-screen tile.

## What it does

- **Overview:** tiles for CPU, memory, GPU, disk, network, temperature and battery,
  each with a live graph, plus uptime, load and the busiest processes by CPU, memory,
  disk or GPU. Choose which tiles show and in what order.
- **Processes:** every process with its CPU, memory, disk, GPU, threads and state.
  - **Views:** *Apps* (grouped under their window), *All*, *Tree* and *Mine*.
  - **Controls:** sort by any column, show, hide, resize or reorder columns (all
    remembered), and filter by name, command, PID or user. Filtering in *Apps* and
    *Tree* keeps the tree, opened down to each match.
  - **Totals:** column headers show the machine's CPU, memory, disk and GPU use, and
    busy cells are shaded.
  - **Actions:** end, kill, pause/resume, change priority, switch to or close the
    window, open its location, show it in the tree, or copy its command line, from the
    details panel or a right-click. Signals and priority go to every selected process.
    Other users' processes go through `pkexec`.
  - **Details:** live CPU and memory graphs, open files, start time and the parent
    process. The panel collapses to one line, and sits beside the table in wide windows.
  - **Keys:** <kbd>Delete</kbd> ends a process and <kbd>Shift</kbd>+<kbd>Delete</kbd>
    kills it. Either needs a second press unless you turn that off in Settings.
    <kbd>←</kbd>/<kbd>→</kbd> close and open branches, and <kbd>/</kbd> filters.
- **Performance pages:**
  - **CPU:** total and kernel time, a graph or a shaded tile per thread, clocks, load
    and the governor.
  - **Memory:** RAM, cache and swap/zram (with its compression ratio).
  - **Storage:** each disk's throughput, how busy it is, its temperature, and how full
    each mount is.
  - **Network:** traffic per interface, addresses, Wi-Fi network and signal.
  - **GPU:** Intel/AMD usage from DRM fdinfo and sysfs, and NVIDIA through `nvidia-smi`,
    plus the processes using each GPU.
  - **Sensors & Power:** every temperature sensor, fans, and battery charge, draw and health.
- **Startup:** what runs at login.
  - Apps from XDG autostart can be turned off with a user override; the system file
    is never touched.
  - Items you add are started by Hyprland from `~/.config/hypr/tasks.lua`.
  - Omarchy's own startup and your `autostart.lua` are shown for reference.
  - Also: user services, and what took longest at your last login.
- **Services:** systemd user and system services and timers. Start, stop, restart,
  enable/disable, see a unit's PID, memory, CPU time and command, edit it, and read
  its logs (filtered by text or priority, or following live). Timers show their next
  and last run. The list refreshes by itself while it's open.
- **Warnings and alerts:** CPU temperature, memory use and disk fullness turn red past
  limits you set, and optional desktop notifications cover those, a process stuck at
  high CPU, and failed services.
- **Go to:** <kbd>Ctrl</kbd>+<kbd>K</kbd> finds any page, process, service or startup
  item.

Graphs keep 1, 5 or 10 minutes of history and update every 0.5–5 s. Hover any graph
to read past values, and right-click one to export it as CSV (Settings exports them
all). <kbd>Ctrl</kbd>+<kbd>P</kbd> pauses updates.

### Easy on the battery

On hybrid laptops the discrete GPU sleeps when nothing uses it. Tasks keeps it asleep:

- It uses GTK's GL renderer, because the Vulkan renderer probes every GPU and wakes it.
- It never runs `lspci`; GPU names come from `pci.ids`.
- It only asks `nvidia-smi` for numbers while the GPU is already awake, and backs off
  once it's idle.

While the window is hidden, the per-process and GPU sampling stops too.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nexus-tasks/main/install.sh | bash
```

This builds with Cargo and installs `tasks` to `~/.local/bin`, along with a launcher
entry. Add `-s -- --bind` after `bash` to also print a binding that opens it with
Ctrl+Shift+Esc:

```lua
o.bind("CTRL + SHIFT + Escape", "Task manager", hl.dsp.exec_cmd("tasks --toggle"))
```

To remove it, run the same line with `uninstall.sh` in place of `install.sh`. Add
`-s -- --purge` to also remove its settings and startup items.

## Usage

```
tasks [--section ID] [--toggle]
```

`--section` opens (or switches the open window to) `overview`, `processes`, `cpu`,
`memory`, `storage`, `network`, `gpu`, `sensors`, `startup`, `services` or `settings`.

## Files

| Path | What |
| --- | --- |
| `~/.config/nexus-tasks/settings.toml` | This app's preferences |
| `~/.config/nexus-tasks/themes/*.toml` | Your own themes |
| `~/.config/nexus-tasks/state.json` | Startup items added in Tasks |
| `~/.config/hypr/tasks.lua` | Generated from the above; loaded by `require("hypr.tasks")` |
| `~/.config/autostart/*.desktop` | XDG autostart overrides (`Hidden=true`) |

## License

MIT
