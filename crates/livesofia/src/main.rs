mod audio;
mod ipc;
mod live;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("livesofia=info")),
        )
        .init();

    dotenvy::from_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env")).ok();
    let hub = livesofia::state::EventHub::new();
    match sofia_trace_store::Store::default_path().and_then(sofia_trace_store::Store::open) {
        Ok(store) => {
            let hub_for_trace = hub.clone();
            let (_, mut events) = hub.subscribe();
            tokio::spawn(async move {
                let mut recorder = sofia_trace_store::Recorder::new(store);
                loop {
                    match events.recv().await {
                        Ok(message) => {
                            if let sofia_protocol::ServerBody::Event(event) = message.body
                                && let Err(error) = recorder
                                    .record(&event, hub_for_trace.snapshot().active_session_id)
                            {
                                tracing::warn!(%error, "could not record trace event");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                            tracing::warn!(count, "trace recorder skipped events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        Err(error) => tracing::warn!(%error, "trace storage unavailable"),
    }
    match sofia_config::load_output_device_id() {
        Ok(device_id) => hub.set_output_device(device_id),
        Err(error) => tracing::warn!(%error, "could not load selected audio output"),
    }
    match sofia_config::load_gemini_voice_name() {
        Ok(Some(name)) => match livesofia::voice::canonical_voice_name(&name) {
            Some(name) => hub.set_gemini_voice(Some(name.into())),
            None => tracing::warn!(voice = %name, "stored Gemini voice is unavailable"),
        },
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "could not load Gemini voice"),
    }
    let (commands, receiver) = tokio::sync::mpsc::channel(64);
    tokio::spawn(live::run(hub.clone(), receiver));
    ipc::run_server(hub, commands).await
}
