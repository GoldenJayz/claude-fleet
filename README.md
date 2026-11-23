# Claude Fleet

A small Windows desktop app that shows **Claude Code usage** and **PC performance** for all your machines in one glass dashboard:

- **Prompt feed** per machine: every prompt you send to Claude Code, with model, effort and token counts, plus today/this-week totals and what Claude is **working on right now**.
- **Plan usage** banner: 5-hour and weekly limits with reset countdowns, and a Windows alert at 80%.
- **Performance** per machine: CPU, RAM, disk space and activity, GPU (NVIDIA, AMD or Intel), network up/down, and temperatures where Windows exposes them.
- **Heavy processes**: anything using a lot of CPU or RAM for 10 s or more, with a safe **End process / End process tree** flow and a kill log.

Everything stays on your machines. There are no cloud services, no telemetry and no Claude API calls. The collector is a plain program that polls on a timer.

---

## How it works

```
 ┌──────────── JadensPC ────────────┐        ┌──────────── Laptop ─────────────┐
 │ Claude Fleet app                 │        │ Claude Fleet app                │
 │  ├─ collector (port 47821) ◄─────┼── LAN ─┼──► dashboard                    │
 │  │   metrics · logs · SQLite     │        │                                 │
 │  └─ dashboard ───────────────────┼── LAN ─┼─► collector (port 47821)        │
 └──────────────────────────────────┘        └─────────────────────────────────┘
```

Each PC runs the same app. Its built-in **collector**:

- reads system metrics and processes every 2 s, and Claude Code's logs (`%USERPROFILE%\.claude\projects\**\*.jsonl`) every 5 s, reading only new bytes;
- keeps history in SQLite (`%APPDATA%\ClaudeFleet\fleet.db`);
- serves a versioned API on port **47821**: `/v1/snapshot`, `/v1/stream` (WebSocket), `/v1/processes/{pid}/kill` and more.
  - Every request needs that PC's **pairing token**.
  - Requests from outside your local network or Tailscale are refused.

The **dashboard** connects to this PC and to every machine in its list (`%APPDATA%\ClaudeFleet\dashboard.json`). Adding a third PC is just another entry in that list, with no code changes.

Closing the window **hides it to the tray**, so the other PCs can keep seeing this one. Use **Quit** in the tray menu to really exit.

---

## Install

1. Copy `Claude Fleet_0.1.0_x64_en-US.msi` to the PC and run it.
2. Open **Claude Fleet** from the Start menu.
3. In **Settings → System**:
   - **Start with Windows**: on, so the collector is always available.
   - **Allow other PCs to connect**: click **Allow** and approve the Windows prompt. This adds a firewall rule for port 47821 that covers this app only, your local subnet and Tailscale (100.64.0.0/10) only, on Private and Domain networks only.
   - **Claude Code status line**: click **Install**. It merges a status-line command into `%USERPROFILE%\.claude\settings.json`, writes a timestamped backup first, and keeps any status line you already had running.

Windows 10/11 x64. WebView2 is preinstalled on Windows 11.

## Pair two machines

On the PC you want to **add** (for example the laptop):

1. **Settings → This PC → Pairing code**: pick the network both PCs share (your Wi-Fi/Ethernet address, or Tailscale), then **Copy code**. The code looks like `claude-fleet://192.168.1.20:47821/<token>?name=Laptop`.
2. Get the code to the other PC privately. It contains the token.

On the PC that should **show** it:

3. **Settings → Machines → + Add machine**, paste the code, then **Test connection** and **Add machine**.

Do the same in the other direction if you want both dashboards to show both PCs. Each card shows **Offline** (with when it was last seen) whenever a machine can't be reached, and reconnects by itself.

If **Test connection** fails:

| Message | Fix |
|---|---|
| *Couldn't reach …* | Is Claude Fleet running on that PC (check the tray)? Did you click **Allow** there? Is that network set to **Private** in Windows (Settings → Network)? Public networks are blocked on purpose. |
| *the pairing token is wrong* | Copy the code again from that PC's Settings. |
| *refused this address* | The address isn't a LAN or Tailscale address. Use one from the pairing-code list. |

## Machines on different networks: Tailscale

If the PCs aren't on the same network (say, the laptop is away from home), use [Tailscale](https://tailscale.com):

1. Install Tailscale on both PCs and sign in to the same account.
2. On each PC, in **Settings → This PC → Pairing code**, choose the **Tailscale** address (100.x.y.z) and copy that code.
3. Add the machines with those codes as above.

The collector accepts Tailscale's 100.64.0.0/10 range, and the firewall rule includes it. Traffic stays inside your tailnet, end-to-end encrypted. Nothing is exposed to the internet.

## Plan usage

There's no public API for plan limits. Claude Code passes them to its **status line** command, so Claude Fleet's status line saves them (`%APPDATA%\ClaudeFleet\statusline.json`) and the banner shows them. Things to know:

- Only **terminal** Claude Code runs the status line. Desktop-app sessions don't, so readings refresh when a terminal Claude Code session replies on any of your PCs.
- Usage is per account and shared with claude.ai and the mobile apps, so the banner shows the newest reading from any machine.
- If a reading is old but its window hasn't reset, it's shown as **"≥ X%"**: usage can only have gone up. If the window has reset since, it says so instead of guessing.
- There's deliberately no estimate from token counts. The logs on your PCs can't see claude.ai or phone usage, and testing showed such estimates were wildly off.

## Ending heavy processes

- **Heavy** means ≥ 20 % of total CPU, or ≥ max(2 GB, 15 % of RAM), in every sample for 10 s or more. A process stays listed while its average holds above 80 % of the threshold.
- **Never ended (protected):**
  - Windows core processes: System, Registry, smss, csrss, wininit, winlogon, services, lsass, lsaiso, dwm, explorer, svchost, fontdrvhost, sihost
  - Claude
  - Claude Fleet itself, including its own window processes
- The confirm dialog shows what ending the process (or its whole tree) would free. **End process tree** ends the children first and leaves protected branches running.
- Every request includes the process start time, so a recycled PID can't hit a different program.
- Claude Fleet never asks for admin rights. If Windows refuses, it tells you to use Task Manager as administrator.
- Every attempt is logged (**Processes → Kill log**) with time, machine, process, who asked and from which IP.

## Privacy and security

- Prompt text stays in `%APPDATA%\ClaudeFleet\fleet.db` on each PC, and is only sent to your own dashboards over your LAN or tailnet.
- The pairing token (64 hex characters) is created on first run and stored in `%APPDATA%\ClaudeFleet\collector.json`. Anyone with it, on your network, can read that PC's stats and prompts and end its non-protected processes, so treat it like a password.
  - To change it, quit the app, set `"token": ""` in that file, and start the app again. A new one is generated, and you'll need to re-pair.
- Saved machines (with their tokens) are in `%APPDATA%\ClaudeFleet\dashboard.json`.
- Traffic is plain HTTP on your local network. Use Tailscale if you need encryption between sites.

## Uninstall

1. In **Settings → System**, click **Remove** next to the status line and next to the firewall rule.
2. Uninstall *Claude Fleet* from Windows Settings → Apps.
3. Optionally delete `%APPDATA%\ClaudeFleet`.

---

## Development

Requirements: Node 20+, Rust (stable, MSVC), Visual Studio C++ build tools, WebView2.

```powershell
npm install
npm run tauri dev        # app with hot reload
npm run tauri build      # installer: target\release\bundle\msi\
cargo test -p fleet-collector
```

Standalone collector (no UI), handy for testing a second machine on one PC:

```powershell
cargo run -p fleet-collector -- --data-dir $env:TEMP\fleet2 --port 47822 --name TestBox run
cargo run -p fleet-collector -- --data-dir $env:TEMP\fleet2 token
```

Layout:

| Path | What |
|---|---|
| `crates/collector` | Collector library and `fleet-collector` CLI: metrics, GPU counters, log reader, SQLite, API, process control, status line |
| `src-tauri` | Tauri app: embeds the collector; window effects, tray, autostart, firewall and status-line commands |
| `src` | React + TypeScript + Tailwind + Framer Motion dashboard |
| `docs/data-sources.md` | Where every number comes from, verified against real logs |
