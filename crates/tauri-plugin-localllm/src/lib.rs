//! Chat model that runs on the phone itself (Android only, LiteRT-LM).
//!
//! The work is done by the Kotlin plugin `pl.lokalsi.localllm.LocalLlmPlugin`;
//! this crate only registers it. On other platforms the plugin does nothing.

use tauri::{
    Runtime,
    plugin::{Builder, TauriPlugin},
};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "pl.lokalsi.localllm";

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("localllm")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin(PLUGIN_IDENTIFIER, "LocalLlmPlugin")?;
            Ok(())
        })
        .build()
}
