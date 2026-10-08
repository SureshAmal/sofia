//! GPUI Kit settings application boundary.
//!
//! The shared settings file API is implemented in `sofia-config`.

pub use sofia_config::{
    load_gemini_voice_name, load_output_device_id, save_gemini_voice_name, save_output_device_id,
    settings_path,
};
