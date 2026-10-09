# Microphone and speaker path

Tested on Linux, 8 October 2026, with the default PipeWire devices, `rodio` 0.22.2, `gemini-live` 0.1.9, and `gemini-3.8-live` through Vertex AI. The [Live API documentation](https://ai.google.dev/gemini-api/docs/live-api/capabilities) specifies 24 kHz output PCM and describes `audioStreamEnd` for a stream using automatic voice activity detection. The [gemini-live session API](https://docs.rs/gemini-live/0.1.9/gemini_live/session/struct.Session.html) accepts raw PCM at a specified input rate; Rodio's [microphone](https://docs.rs/rodio/0.22.2/rodio/microphone/index.html) and [speaker](https://docs.rs/rodio/0.22.2/rodio/stream/index.html) APIs provide the device streams.

## Observed results

| Check | Result |
| --- | --- |
| Default microphone open through IPC `start_listening` | Accepted; snapshot changed to microphone active |
| Microphone PCM flow | Eight subscribed `input_audio_level` events observed; maximum RMS was 0.153 on the tested device |
| IPC `stop_listening` | Accepted; capture stopped and Live received its audio-stream-end signal |
| Gemini text turn with speaker enabled | Final transcription received: “Hello, it's nice to meet you!” |
| Speaker device | PipeWire listed an active sink input with `application.process.binary = "livesofia"`; the daemon reported no playback error |
| Speaker mute | `set_speaker_muted(true)` and `false` were accepted, and both values appeared in subsequent snapshots |
| Output picker | `list_audio_outputs` returned device IDs and names; `select_audio_output` opened a chosen device and saved its ID in an isolated `setting.json` |
| Restart and invalid ID | The chosen output returned in the snapshot after a daemon restart; an invalid ID was rejected without changing it |
| Gemini voice list | `list_gemini_voices` returned 30 named prebuilt voices and the selected voice |
| Gemini voice change | Selecting `Kore` was accepted, stored in an isolated `setting.json`, and the Live session reconnected to `ready` with `Kore` in its snapshot |
| Spoken answer with selected voice | `ipc_probe` received text deltas and a final transcription, “Hello, I hope you are having a wonderful day!”, while the selected-voice session was active |
| Voice picker and validation | The CLI picker saved `Fenrir` and the session reached `ready` again; an unknown voice was rejected without changing the selection |

The captured audio is downmixed to mono, encoded as signed 16-bit little-endian PCM, and sent with the device's native sample rate. The microphone worker sends bounded 40 ms chunks; the IPC level event is emitted every 80 ms. On stop, the worker finishes and queued chunks are sent before `audio_stream_end`. Model audio is converted from 24 kHz PCM to Rodio's sample source, and Rodio adapts it to the output device. Raw audio stays outside the JSON IPC stream.

## Verification limit

The machine's microphone produced nonzero samples and the operating system accepted Sofia's speaker stream. The [voice probe](crates/livesofia/examples/voice_probe.rs) lets a person verify audible playback and speech recognition; the [output picker](crates/livesofia/examples/audio_output_picker.rs) and [Gemini voice picker](crates/livesofia/examples/gemini_voice_picker.rs) expose the choices until the GPUI settings screen is built. The voice names follow Google's [prebuilt voice list](https://ai.google.dev/gemini-api/docs/speech-generation), and the selected name is sent in Live setup's `speechConfig`. An automated test cannot confirm what a person hears, whether their speech is transcribed accurately, or how the speaker and microphone behave together in a noisy room. Device changes, acoustic echo cancellation, and Windows hardware still need hands-on checks.
