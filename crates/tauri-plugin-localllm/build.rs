// The commands are implemented in Kotlin (android/…/LocalLlmPlugin.kt); the page calls
// them as `plugin:localllm|<command>`.
const COMMANDS: &[&str] = &["status", "load", "generate", "cancel", "unload"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
