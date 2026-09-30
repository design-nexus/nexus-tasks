# Tasks

A task manager for [Omarchy](https://omarchy.org). It takes its colours from your
Omarchy theme and fits a half-screen tile.

## What it does

- **Overview:** tiles for CPU, memory, GPU, disk, network, temperature and battery,
  each with a live graph, plus uptime, load and the busiest processes.
- **Processes:** every process with its CPU, memory, disk, GPU, threads and state.
  - **Views:** *Apps* (grouped under their window), *All*, *Tree* and *Mine*.
  - **Controls:** sort by any column, show or hide columns, and filter by name,
    command, PID or user.
  - **Actions:** end, kill, pause/resume, change priority, switch to or close the
    window, open its location, or copy its command line. Other users' processes go
    through `pkexec`.
  - **Keys:** <kbd>Delete</kbd> ends a process and <kbd>Shift</kbd>+<kbd>Delete</kbd>
    kills it. Either needs a second press unless you turn that off in Settings.
- **Performance pages:**
  - **CPU:** total and kernel time, a graph per thread, clocks, load and the governor.
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
- **Services:** systemd user and system services. Start, stop, restart,
  enable/disable, and read their logs.

Graphs keep 1, 5 or 10 minutes of history and update every 0.5–5 s. Hover a graph
to read past values. <kbd>Ctrl</kbd>+<kbd>P</kbd> pauses updates.

### Easy on the battery

On hybrid laptops the discrete GPU sleeps when nothing uses it. Tasks keeps it asleep:

- It uses GTK's GL renderer, because the Vulkan renderer probes every GPU and wakes it.
- It never runs `lspci`; GPU names come from `pci.ids`.
- It only asks `nvidia-smi` for numbers while the GPU is already awake, and backs off
  once it's idle.

While the window is hidden, the per-process and GPU sampling stops too.

## Install

```sh
git clone https://github.com/design-nexus/nexus-tasks
cd nexus-tasks
./install.sh --bind
```

This builds with Cargo and installs `tasks` to `~/.local/bin`, along with a launcher
entry. `--bind` prints a binding to open it with Ctrl+Shift+Esc:

```lua
o.bind("CTRL + SHIFT + Escape", "Task manager", hl.dsp.exec_cmd("tasks --toggle"))
```

`./uninstall.sh` removes it. `--purge` also removes its settings and startup items.

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
