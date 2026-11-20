//! Default name for the pairing screen: the device name the owner set in
//! Android settings (Settings.Global.DEVICE_NAME) and the model (Build.MODEL),
//! e.g. "Telefon Łukasza (Pixel 8a)". `None` when nothing can be read; the UI
//! then keeps its generic default.

/// Joins the owner-set name and the model without repeating the model.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn label(name: Option<&str>, model: Option<&str>) -> Option<String> {
    let name = name.map(str::trim).filter(|s| !s.is_empty());
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    match (name, model) {
        (Some(n), Some(m)) if n.to_lowercase().contains(&m.to_lowercase()) => Some(n.to_string()),
        (Some(n), Some(m)) => Some(format!("{n} ({m})")),
        (Some(n), None) => Some(n.to_string()),
        (None, Some(m)) => Some(m.to_string()),
        (None, None) => None,
    }
}

#[cfg(target_os = "android")]
pub fn read() -> Option<String> {
    use jni::JavaVM;
    use jni::objects::{JObject, JString};

    let ctx = ndk_context::android_context();
    // SAFETY: the pointers come from the Android runtime that started the app
    // and stay valid for its lifetime; we never delete the context reference.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    fn string_of(env: &mut jni::JNIEnv, obj: JObject) -> Option<String> {
        if obj.is_null() {
            return None;
        }
        let s: String = env.get_string(&JString::from(obj)).ok()?.into();
        Some(s)
    }

    let model = env
        .get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")
        .and_then(|v| v.l())
        .ok()
        .and_then(|o| string_of(&mut env, o));

    let name = (|| {
        let resolver = env
            .call_method(
                &context,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        let key = env.new_string("device_name").ok()?;
        let value = env
            .call_static_method(
                "android/provider/Settings$Global",
                "getString",
                "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;",
                &[(&resolver).into(), (&key).into()],
            )
            .ok()?
            .l()
            .ok()?;
        string_of(&mut env, value)
    })();

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    tracing::debug!(
        has_name = name.is_some(),
        has_model = model.is_some(),
        "device name read"
    );
    label(name.as_deref(), model.as_deref())
}

#[cfg(not(target_os = "android"))]
pub fn read() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::label;

    #[test]
    fn joins_name_and_model() {
        assert_eq!(
            label(Some("Telefon Łukasza"), Some("Pixel 8a")).as_deref(),
            Some("Telefon Łukasza (Pixel 8a)")
        );
    }

    #[test]
    fn does_not_repeat_the_model() {
        assert_eq!(
            label(Some("Pixel 8a"), Some("Pixel 8a")).as_deref(),
            Some("Pixel 8a")
        );
        assert_eq!(
            label(Some("Google Pixel 8a"), Some("pixel 8a")).as_deref(),
            Some("Google Pixel 8a")
        );
    }

    #[test]
    fn falls_back_to_what_is_there() {
        assert_eq!(
            label(Some("  "), Some("Pixel 8a")).as_deref(),
            Some("Pixel 8a")
        );
        assert_eq!(label(Some("Mój"), None).as_deref(), Some("Mój"));
        assert_eq!(label(None, None), None);
    }
}
