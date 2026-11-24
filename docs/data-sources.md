# Data sources (verified on DesktopPC, Claude Code 2.1.318, Nov 2026)

## Claude Code logs

`%USERPROFILE%\.claude\projects\<project-slug>\<session-id>.jsonl` (or `CLAUDE_CONFIG_DIR`), one JSON object per line.
Subagent logs: `<project-slug>\<session-id>\subagents\agent-*.jsonl`. They carry the **parent's** `sessionId` and `isSidechain: true`.

### Responses: `type: "assistant"`

| What | Where |
|---|---|
| model | `message.model` (`claude-opus-5-5`, `claude-haiku-5-5`, `claude-fable-5-2`, …). Skip `<synthetic>`. |
| tokens | `message.usage.input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens_details.thinking_tokens` (thinking is part of output) |
| effort | top-level `effort` and `perTurnEffort` (`"high"`, `"medium"`, …) |
| dedupe key | `message.id`: **one line per content block, each repeating the same usage**. Summing lines overcounts about 1.65×. |

Request lines (`type: "api-request"`) also carry `params.output_config.effort`.

### Prompts: `type: "user"`

A prompt you typed has `origin.kind: "human"`. Older lines only have `turnOrigin: "human"`. Skip these:
`isMeta: true`, `isSidechain: true`, content arrays containing `tool_result`, `origin.kind` of `peer` / `task-notification`,
and text that starts with injected envelopes (`<local-command-stdout>`, `<ci-monitor-event>`, `<task-notification>`, …).
`<command-name>/x</command-name>` lines are slash commands. `<system-reminder>…</system-reminder>` blocks are stripped.

Assistant lines don't reference their prompt. A response belongs to the latest prompt in the same session with `ts <=` the response's timestamp.

### Default effort

`settings.json` → `effortLevel` (here `"medium"`); per-model overrides in `modelSettings` if present (none on this PC).
The desktop app usually runs at `high` regardless, so the per-line `effort` is what the feed shows.

## Plan usage (status line)

Claude Code pipes JSON to `statusLine.command` on stdin. For subscribers it includes:

```json
"rate_limits": {
  "five_hour": { "used_percentage": 69, "resets_at": 1795462200 },
  "seven_day": { "used_percentage": 33, "resets_at": 1795867200 }
}
```

The block is present only after the first API response of a session, and only while a window's `resets_at` hasn't passed.
On Windows the command runs through **Git Bash**: use forward-slash paths.
Captured live with a temporary `--settings` file. The user's settings were left untouched.

`crates/collector/assets/statusline.sh` saves captures that include `rate_limits` to `%APPDATA%\ClaudeFleet\statusline.json`.

## System metrics

- CPU, RAM, disks (space + read/write bytes), network, processes: `sysinfo` 0.40.
- GPU: sysinfo's Windows VRAM number is the *calling process's* usage, so `src/gpu.rs` reads the
  `GPU Engine` / `GPU Adapter Memory` performance counters per adapter LUID (the Task Manager method; works for AMD, Intel and NVIDIA).
- Temperatures: sysinfo Components (WMI; usually empty without admin rights) plus NVML for NVIDIA GPUs. DesktopPC has an AMD GPU and reports no temperatures.
