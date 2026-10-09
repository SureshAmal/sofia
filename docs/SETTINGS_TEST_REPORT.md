# Settings, system prompt, and audio fixes

Test environment: Linux, Hyprland/Wayland, PipeWire, Vertex Gemini Live. Date: 2026-10-09.

## Implemented

- GPUI Kit native Settings pages for Assistant, Connection, Voice & audio, and Appearance. The library owns navigation, search, groups, and reset controls.
- Editable system instruction, sent in Gemini setup, with a concise voice-oriented default and reset support.
- Vertex project, location, model, OAuth client ID/secret/refresh token; blank credentials fall back to the existing `.env`.
- Speaker picker, voice setting, and automatic listening preference.
- Voice is a selectable list of all 30 prebuilt voices plus provider default.
- Provider switch: Vertex OAuth versus Gemini API key; inactive provider credentials remain saved. An API key selects Gemini API when no explicit toggle choice is saved. Gemini API uses `Endpoint::GeminiApi` with `Auth::ApiKey` and a `models/...` model resource; Vertex retains its project resource and refreshable bearer provider.
- Atomic JSON storage preserving unrelated fields; Linux folder/file permissions are 0700/0600. Credentials are stored as JSON, not in an OS keychain yet.
- Save & apply sends IPC `reload_settings` and reconnects the model to apply settings. IPC version is now 8; run matching daemon and client binaries.
- Bundled GPUI Kit Assets registered, including navigation, search, reset, and dropdown icons.
- System theme is the default. `Theme::sync_system_appearance(Some(window), cx)` and a window appearance observer follow OS changes. Explicit light/dark/custom preferences remain explicit. `ThemeRegistry::watch_dir` handles custom files.

## Audio corrections

The previous 32-chunk speaker limit counted sounds, not duration. Gemini audio may arrive in bursts faster than playback, so valid responses exceeded the limit. Playback now tracks outstanding samples and permits at most 60 seconds of queued PCM. Consumed and cancelled samples release the counter.

Right-click previously ended microphone input and switched to Thinking. It now pauses microphone and speaker, clears playback, stays Ready, and ignores pending model responses until resumed. Duplicate pause requests are accepted.

## Verified

- Live IPC pause: Listening → Ready; microphone false; still Ready after three seconds.
- Live reload: accepted; a new Gemini session connected and microphone capture resumed automatically.
- Unit coverage checks the exact system instruction in Gemini setup, queue accounting on consumption/cancellation, configuration defaults and preservation, and presence of bundled settings icons.
- Linux portal reports dark preference (`color-scheme = 1`). System theme now uses GPUI Kit's window-specific Linux path.

## Remaining scope

Windows is not runtime-tested here. Gemini API routing is unit-tested; a live API-key connection requires a valid key and a model available on that endpoint. Vertex Live has been tested separately. Analytics, history/memory storage, user-content databases, PostgreSQL sync, non-Google provider adapters, and MCP tools are not implemented by this settings milestone.
