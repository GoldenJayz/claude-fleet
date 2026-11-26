# Pane

*Formerly Claude Fleet. Some internal names keep the old one so existing installs update in place and stay paired: the settings folder `%APPDATA%\ClaudeFleet`, `claude-fleet://` pairing codes and the program name `claude-fleet.exe`.*

A small Windows desktop app that shows **Claude Code usage** and **PC performance** for all your machines in one glass dashboard:

- **Prompt feed** per machine: every prompt you send to Claude Code, with model, effort and token counts, plus today/this-week totals and what Claude is **working on right now**.
- **Plan usage** banner: 5-hour and weekly limits from Anthropic, with reset countdowns and a Windows alert at 80%.
- **Performance** per machine: CPU, RAM, disk space and activity, GPU (NVIDIA, AMD or Intel), network up/down, and temperatures where Windows exposes them.
- **Heavy processes**: anything using a lot of CPU or RAM for 10 s or more, with a safe **End process / End process tree** flow and a kill log.
- **File transfer**: drag files onto the window and drop them on another PC.
- **Remote terminal** (opt-in): a PowerShell on another PC, right in the dashboard.
- **Agents** (opt-in): hand Claude Code a task to run on one of your PCs, watch it work, queue follow-ups.
- **Screenshots** (opt-in): see another PC's screen from its page.
- **Android app**: the same dashboard on your phone (see [Phone app](#phone-app-android)).

Everything stays on your machines: no cloud services and no telemetry. The only outside calls are asking Anthropic for your plan usage (see [Plan usage](#plan-usage)) and checking GitHub for app updates (see [Updates](#updates)). The collector is a plain program that polls on a timer.

---

## How it works

```
 ┌──────────── DesktopPC ────────────┐        ┌──────────── Laptop ─────────────┐
 │ Pane app                 │        │ Pane app                │
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

1. Copy `Pane_0.1.0_x64_en-US.msi` to the PC and run it.
2. Open **Pane** from the Start menu.
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
| *Couldn't reach …* | Is Pane running on that PC (check the tray)? Did you click **Allow** there? Is that network set to **Private** in Windows (Settings → Network)? Public networks are blocked on purpose. |
| *the pairing token is wrong* | Copy the code again from that PC's Settings. |
| *refused this address* | The address isn't a LAN or Tailscale address. Use one from the pairing-code list. |

## Machines on different networks: Tailscale

If the PCs aren't on the same network (say, the laptop is away from home), use [Tailscale](https://tailscale.com):

1. Install Tailscale on both PCs and sign in to the same account.
2. On each PC, in **Settings → This PC → Pairing code**, choose the **Tailscale** address (100.x.y.z) and copy that code.
3. Add the machines with those codes as above.

The collector accepts Tailscale's 100.64.0.0/10 range, and the firewall rule includes it. Traffic stays inside your tailnet, end-to-end encrypted. Nothing is exposed to the internet.

## Plan usage

The banner shows your 5-hour and weekly limits: the same numbers as Claude Code's `/usage`.

- Every 5 minutes, each PC asks Anthropic for them, using the sign-in Claude Code already saved on that PC (`~/.claude/.credentials.json`). This is the only call Pane makes outside your network. It sends nothing but that request: no prompts and no logs.
- Pane only reads that sign-in; it never renews or changes it. When it's about to expire, Pane runs `claude auth status`, which makes Claude Code renew it the normal way.
- The **status line** is still used as a backup. If you install it (Settings → System), terminal Claude Code sessions also hand over their numbers, and the newest reading wins.
- Usage is per account and shared with claude.ai, the apps and all your PCs, so the banner shows the newest reading from any machine.
- If no reading is newer than 10 minutes (for example, every PC is offline), the last one is shown as **"≥ X%"**, since usage can only have gone up. If the window has reset since, it says so instead of guessing.
- There's deliberately no estimate from token counts. The logs on your PCs can't see claude.ai or phone usage, and testing showed such estimates were wildly off.

## Ending heavy processes

- **Heavy** means ≥ 20 % of total CPU, or ≥ max(2 GB, 15 % of RAM), in every sample for 10 s or more. A process stays listed while its average holds above 80 % of the threshold.
- **Never ended (protected):**
  - Windows core processes: System, Registry, smss, csrss, wininit, winlogon, services, lsass, lsaiso, dwm, explorer, svchost, fontdrvhost, sihost
  - Claude
  - Pane itself, including its own window processes
- The confirm dialog shows what ending the process (or its whole tree) would free. **End process tree** ends the children first and leaves protected branches running.
- Every request includes the process start time, so a recycled PID can't hit a different program.
- Pane never asks for admin rights. If Windows refuses, it tells you to use Task Manager as administrator.
- Every attempt is logged (**Processes → Kill log**) with time, machine, process, who asked and from which IP.

## Sending files

Drag files from Explorer anywhere onto the Pane window. Each of your other PCs appears as a drop target: drop on one to send. You can also use **Send files** on a PC's page.

- Files are saved in that PC's **Downloads\Pane** folder (change it with `"inbox_dir"` in `%APPDATA%\ClaudeFleet\collector.json`). If a file with that name exists, the new one is saved as `name (2).ext`.
- The receiving PC shows a Windows notification. The **Transfers** panel (bottom right) shows progress and lets you cancel; on the receiving PC, **Show** opens the file in Explorer.
- Files are marked as coming from another computer, like a browser download, so Windows asks before running a received program.
- Nothing is saved until the whole file has arrived, and a transfer that would leave less than 1 GB free on that PC is refused.
- Folders can't be sent yet: zip them first (right-click → Compress to ZIP file).

## Remote terminal

**Terminal** on another PC's page opens an interactive PowerShell on that PC (PowerShell 7 if installed), so you can run commands, including `claude`, without walking over to it. The session stays open while you look at other pages; it's listed under **Terminals** in the sidebar. **End session** closes the shell and anything still running in it.

It's **off by default**. Turn it on, on the PC you want to reach, in **Settings → System → Allow remote shell**. There's no way to turn it on from another PC.

- The shell runs as the signed-in user, never as admin.
- The PC being used gets a Windows notification and a banner for as long as a session is open. Every session is logged (`shell_log` in `fleet.db`).
- At most 4 sessions at once.
- Anyone with that PC's pairing code on your network can then run any command there as you. Traffic on a local network isn't encrypted, so only turn it on for networks you trust, or use Tailscale.

## Agents

**Agents** (sidebar, or the tab bar on the phone) hands a task to Claude Code on one of your PCs. Pick the PC, a folder on it (recent Claude Code folders are suggested), a permission mode and optionally a model, then type what you want done. That PC runs Claude Code headless (`claude -p`) in that folder, with its own Claude sign-in, and every dashboard shows the progress live: what Claude says and each tool it uses.

- **Queue messages:** tasks run one at a time per PC; send more and they wait their turn. Open a task to **queue a follow-up**: it continues the same Claude conversation once the previous message is done. **Stop** ends a running task; **Remove** drops a queued one.
- **Permission modes:** *Read-only* (plan; changes nothing), *Edit files* (file edits allowed, commands that need approval are skipped), *Auto* (Claude Code's auto mode) and *Full access* (runs anything without asking).
- You get a notification on the device you sent it from when your task finishes or fails.
- **Off by default.** On the PC that should take tasks: **Settings → System → Agent tasks** → *On* (read-only, edit files, auto) or *Full access* (also lets tasks run any command). It can only be turned on from that PC.
- The task text goes to Claude Code on stdin, never on a command line. Tasks are logged in `agent_tasks` in `fleet.db`, and they use that PC's Claude plan.

## Screenshots

**Screen** on another PC's page shows that PC's screen (all monitors side by side). **Auto** takes a new one every 3 seconds; **Save** downloads it. It's **off by default**: turn it on, on the PC to be viewed, in **Settings → System → Allow screenshots**. That PC gets a notification when someone looks (at most once every 10 minutes per viewer).

## Phone app (Android)

The phone app is the dashboard only: it shows your PCs, plan usage, prompts and processes, can end heavy processes, send files and open a remote terminal. It runs no collector, so the phone itself doesn't appear as a machine.

**Add a PC:** on the PC, open **Settings → This PC → Copy code**, get that line to the phone (for example with Phone Link's shared clipboard), then on the phone tap **Settings → Add machine** and paste it. The phone must be on the same Wi-Fi as the PC, or on Tailscale when you're away.

- The PC's firewall rule must allow the collector's port (Settings → System on the PC).
- Pairing codes are kept in the app's private storage on the phone.
- On a phone the terminal gets a row of extra keys (Esc, Tab, Ctrl+C, arrows, Paste).
- Back returns to the overview.

**Home screen widget:** long-press the home screen → **Widgets** → **Pane**. It lists your PCs with online status, CPU and RAM, whether Claude is working (or agent tasks are queued), and the plan usage. Tap it to open the app; the ↻ button refreshes. It refreshes by itself every 30 minutes and whenever you change the PC list in the app, asking the PCs directly over the same network as the app (open the app once after installing so the widget gets your PC list). It can be resized, including on a foldable's cover screen.

**Build and install** (needs Android Studio's SDK and NDK, and the Rust target `aarch64-linux-android`):

```powershell
.\scripts\build-android.ps1            # builds target\android\ClaudeFleet-arm64.apk
.\scripts\build-android.ps1 -Install   # ...and installs it on the phone plugged in over USB
```

For `-Install`, turn on USB debugging on the phone (Settings → About phone → Software information → tap **Build number** 7 times, then Settings → Developer options → **USB debugging**), plug it in and accept the prompt on the phone.

The release build is signed with a key created on this PC in `%APPDATA%\ClaudeFleet\android-signing` (not in the repo). Keep that folder: Android only installs updates signed with the same key.

## Updates

Every copy of Pane (Windows and the phone app) checks this repo's GitHub releases a few seconds after it starts and every 6 hours after that. When there's a new version, a banner offers to install it; **Settings → About → Check for updates** checks right away.

- **Windows:** the update is downloaded, its signature is checked, and it installs and restarts (Windows asks for permission, as with the original install).
- **Phone:** the APK is downloaded and checked against the release's SHA-256, then Android asks you to confirm. The first time, Android asks you to allow "Install unknown apps" for Pane.
- Updates are signed with keys that exist only on the PC that builds releases. Windows refuses an update without a valid signature, and Android refuses an APK signed with a different key, so a tampered release can't install.
- The check sends nothing about you or your PCs: it's a plain download of `latest.json` from GitHub.

## Privacy and security

- Prompt text stays in `%APPDATA%\ClaudeFleet\fleet.db` on each PC, and is only sent to your own dashboards over your LAN or tailnet.
- The pairing token (64 hex characters) is created on first run and stored in `%APPDATA%\ClaudeFleet\collector.json`. Anyone with it, on your network, can read that PC's stats and prompts and end its non-protected processes, so treat it like a password.
  - To change it, quit the app, set `"token": ""` in that file, and start the app again. A new one is generated, and you'll need to re-pair.
- Saved machines (with their tokens) are in `%APPDATA%\ClaudeFleet\dashboard.json`, or in the phone app's private storage.
- Traffic is plain HTTP on your local network, including files you send and what you type in a remote terminal. Use Tailscale if you need encryption.
- The dashboard stops redrawing while it's hidden in the tray, so it uses almost no CPU there; collecting continues.

## Uninstall

1. In **Settings → System**, click **Remove** next to the status line and next to the firewall rule.
2. Uninstall *Pane* from Windows Settings → Apps.
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

Publish a release (all installed copies then offer it):

```powershell
.\scripts\release.ps1 -Version 0.2.1 -Notes "What changed"
```

It bumps the version, builds the signed Windows installer and the Android APK, commits and pushes the version bump, and creates the GitHub release with `latest.json`. It needs a clean working tree, `gh` signed in, and the signing keys in `%APPDATA%\ClaudeFleet\updater-signing` and `android-signing`. **Back up those two folders**: without them, installed copies can't be updated (you'd have to reinstall by hand).

Standalone collector (no UI), handy for testing a second machine on one PC:

```powershell
cargo run -p fleet-collector -- --data-dir $env:TEMP\fleet2 --port 47822 --name TestBox run
cargo run -p fleet-collector -- --data-dir $env:TEMP\fleet2 token
```

Layout:

| Path | What |
|---|---|
| `crates/collector` | Collector library and `fleet-collector` CLI: metrics, GPU counters, log reader, SQLite, API, process control, status line, file receiving, remote shell, agent tasks, screenshots |
| `src-tauri` | Tauri app. Windows (`desktop.rs`): embeds the collector; window effects, tray, autostart, firewall and status-line commands. Android (`mobile.rs`): the dashboard only |
| `src-tauri/gen/android` | Android project (Gradle) for the phone app |
| `scripts/build-android.ps1` | Builds (and optionally installs) the phone app |
| `scripts/release.ps1` | Builds both apps and publishes a GitHub release for the auto-updater |
| `src` | React + TypeScript + Tailwind + Framer Motion dashboard |
| `docs/data-sources.md` | Where every number comes from, verified against real logs |
