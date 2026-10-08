# Gemini Live tool, image, and video probes

Tested: 8 October 2026 with `gemini-live` 0.1.9, `gemini-3.8-live`, the Vertex AI WebSocket in `us-central1`, and refreshed OAuth credentials from the ignored `.env` file. No credentials, image data, or audio bytes were printed.

## Results

| Probe | Sent | Observed | Result |
| --- | --- | --- | --- |
| Function call | `lookup_demo_object` declaration; user request; JSON tool response keyed by call ID | One model tool call, followed by 198,720 bytes of audio and a transcription describing the tool's red circle on the left | Passed |
| Still image | One 320×240 PNG frame over `send_video`, a one-second pause, then a text prompt over real-time input | 227,042 bytes of audio; “On the left, there is a red circle, and on the right, there is a blue square.” | Passed |
| Video frames | Three JPEG frames extracted from a short MP4 and sent through `send_video` at 1 fps, then a text prompt | 264,002 bytes of audio; “The red circle starts on the left side of the screen, moves to the center, and ends on the right side.” | Passed |

The first still-image attempt sent its prompt with `send_client_content` after a `send_video` frame. The model replied that it could not see the image. Sending the prompt through `send_text` on the real-time input channel after the frame produced the correct description. After moving the probe into `livesofia`, one immediate `send_text` attempt also missed the image; a one-second pause between frame and prompt passed. This indicates timing sensitivity in this route, so the delay remains in the probe. The first video run described the final object as centered; a one-second pause after the last frame produced the correct left-to-right description. The test checks for the fixture's visible colors or motion instead of treating any spoken response as success.

## Run it

Create the generated media once, then run each probe:

```sh
bash crates/livesofia/scripts/create_multimodal_fixtures.sh
cargo run -p livesofia --example live_multimodal_probe -- tools
cargo run -p livesofia --example live_multimodal_probe -- image
cargo run -p livesofia --example live_multimodal_probe -- video
```

The image and video probes can also take a path after the mode, for example `cargo run -p livesofia --example live_multimodal_probe -- image /path/to/photo.jpg`. The video probe requires `ffmpeg` and decodes at most five frames at 1 fps; the fixture script also requires ImageMagick. Frame payloads are limited to 200 KB in the probe. The working code is [live_multimodal_probe.rs](crates/livesofia/examples/live_multimodal_probe.rs).

## Scope

The MP4 is decoded locally before transmission; JPEG frames are then sent into the active Live session at 1 fps. The description is requested **after** those frames are streamed. This verifies the live frame transport and temporal visual understanding. It does not yet verify ongoing speech while a camera or screen stream remains active. Google documents a maximum of one video frame per second for Live input, and a video frame alone does not necessarily start a reasoning turn; Sofia will need an explicit text or audio trigger for continuous narration. [Live API capabilities](https://ai.google.dev/gemini-api/docs/live-api/capabilities), [robotics streaming note](https://ai.google.dev/gemini-api/docs/robotics-streaming).

The tool probe verifies declaration, call ID, JSON response, and resumed model speech. It does not yet test parallel calls, cancellation, side-effect authorization, MCP binding, or tool error paths. Those belong to `livesofia` middleware before user-facing tools are enabled. The model supports asynchronous function calling, so the probe waits beyond an initial `TurnComplete` when a tool response has been sent. [Gemini 3.8 Live model details](https://ai.google.dev/gemini-api/docs/models/gemini-3.8-live), [Live API capabilities](https://ai.google.dev/gemini-api/docs/live-api/capabilities).
