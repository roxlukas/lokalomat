//! Launcher icon that follows the edition of the paired computer: purple
//! (`.LauncherFree`, the default) when no computer is paired or it runs the
//! free Lokalomat, gold (`.LauncherPro`) once it runs Lokalomat Pro.
//!
//! Both are `<activity-alias>` entries of `MainActivity` in
//! `AndroidManifest.xml`; exactly one is enabled. Switching goes through
//! `PackageManager.setComponentEnabledSetting(…, DONT_KILL_APP)` and is only
//! done when the wanted alias is not enabled yet. The UI asks for it when the
//! app goes to the background, so the launcher refresh never interrupts work.

/// Alias class names, relative to the manifest namespace `pl.lokalsi.mobile`.
const FREE: &str = "pl.lokalsi.mobile.LauncherFree";
const PRO: &str = "pl.lokalsi.mobile.LauncherPro";

/// `(alias to enable, alias to disable)` for the edition.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn aliases(pro: bool) -> (&'static str, &'static str) {
    if pro { (PRO, FREE) } else { (FREE, PRO) }
}

#[cfg(not(target_os = "android"))]
pub fn set(pro: bool) {
    tracing::debug!(pro, "launcher icon: nothing to switch on this platform");
}

#[cfg(target_os = "android")]
pub fn set(pro: bool) {
    match switch(pro) {
        Ok(true) => tracing::info!(pro, "launcher icon switched"),
        Ok(false) => tracing::debug!(pro, "launcher icon already right"),
        Err(e) => tracing::warn!(pro, error = %e, "launcher icon not switched"),
    }
}

/// `PackageManager` constants.
#[cfg(target_os = "android")]
const STATE_DEFAULT: i32 = 0;
#[cfg(target_os = "android")]
const STATE_ENABLED: i32 = 1;
#[cfg(target_os = "android")]
const STATE_DISABLED: i32 = 2;
#[cfg(target_os = "android")]
const DONT_KILL_APP: i32 = 1;

/// Returns whether anything changed.
#[cfg(target_os = "android")]
fn switch(pro: bool) -> jni::errors::Result<bool> {
    use jni::JavaVM;
    use jni::objects::JObject;

    let ctx = ndk_context::android_context();
    // SAFETY: the pointers come from the Android runtime that started the app
    // and stay valid for its lifetime; we never delete the context reference.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };
    let result = switch_with(&mut env, &context, pro);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result
}

#[cfg(target_os = "android")]
fn switch_with(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
    pro: bool,
) -> jni::errors::Result<bool> {
    let pm = env
        .call_method(
            context,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )?
        .l()?;
    let package = env
        .call_method(context, "getPackageName", "()Ljava/lang/String;", &[])?
        .l()?;
    let (on, off) = aliases(pro);
    let on_c = component(env, &package, on)?;
    let off_c = component(env, &package, off)?;

    // DEFAULT means the manifest value: LauncherFree enabled, LauncherPro disabled.
    let enabled = |s: i32, alias: &str| s == STATE_ENABLED || (s == STATE_DEFAULT && alias == FREE);
    let on_enabled = enabled(state(env, &pm, &on_c)?, on);
    let off_enabled = enabled(state(env, &pm, &off_c)?, off);
    if on_enabled && !off_enabled {
        return Ok(false);
    }
    // Enable the new entry first, so the launcher never has none.
    set_state(env, &pm, &on_c, STATE_ENABLED)?;
    set_state(env, &pm, &off_c, STATE_DISABLED)?;
    Ok(true)
}

#[cfg(target_os = "android")]
fn component<'l>(
    env: &mut jni::JNIEnv<'l>,
    package: &jni::objects::JObject,
    class: &str,
) -> jni::errors::Result<jni::objects::JObject<'l>> {
    let class = env.new_string(class)?;
    env.new_object(
        "android/content/ComponentName",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[package.into(), (&class).into()],
    )
}

#[cfg(target_os = "android")]
fn state(
    env: &mut jni::JNIEnv,
    pm: &jni::objects::JObject,
    component: &jni::objects::JObject,
) -> jni::errors::Result<i32> {
    env.call_method(
        pm,
        "getComponentEnabledSetting",
        "(Landroid/content/ComponentName;)I",
        &[component.into()],
    )?
    .i()
}

#[cfg(target_os = "android")]
fn set_state(
    env: &mut jni::JNIEnv,
    pm: &jni::objects::JObject,
    component: &jni::objects::JObject,
    value: i32,
) -> jni::errors::Result<()> {
    env.call_method(
        pm,
        "setComponentEnabledSetting",
        "(Landroid/content/ComponentName;II)V",
        &[
            component.into(),
            jni::objects::JValue::Int(value),
            jni::objects::JValue::Int(DONT_KILL_APP),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_enables_the_gold_alias() {
        assert_eq!(aliases(true), (PRO, FREE));
        assert_eq!(aliases(false), (FREE, PRO));
    }
}
