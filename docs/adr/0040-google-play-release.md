# ADR 0040: Google Play release – upload key, AAB and "Zgłoś" on AI answers

- Status: accepted
- Date: 2026-11-26

## Context

The owner wants to publish the Android app on Google Play. Until now only debug
APKs were built (ADR 0020), signed with the Android debug key. Google Play needs:
an Android App Bundle (AAB) signed with the developer's upload key (Play App
Signing re-signs it), and, for apps with generative AI, a way to report offensive
AI output from inside the app (AI-Generated Content policy).

## Decision

- **Upload key:** a PKCS12 keystore (RSA 4096, alias `upload`, valid until 2054)
  kept on the release PC outside the repo, its password in a file next to it. Never
  in a repo; keep a backup outside the PC. If it is lost, Google can reset the
  upload key (Play App Signing holds the app signing key).
- **Gradle:** `apps/mobile/src-tauri/gen/android/app/build.gradle.kts` signs the
  release build when a `keystore.properties` exists, either at the path in
  `LOKALOMAT_KEYSTORE_PROPERTIES` or next to the Gradle project (git-ignored). It
  names `storeFile`, `keyAlias` and `passwordFile`, so the password is never in a
  properties file or an environment variable. Without it the release build stays
  unsigned, so contributors can still build.
- **Bundle:** `npm run tauri android build -- --aab --target aarch64` in
  `apps/mobile` with `LOKALOMAT_KEYSTORE_PROPERTIES=<key folder>/keystore.properties`.
  arm64 only, because the on-phone model engine (LiteRT-LM, ADR 0038) and nearly
  every current phone are arm64.
- **"Zgłoś" (`ui/src/mobile/ReportAnswer.svelte`):** under every AI answer in both
  phone chats. It opens the mail app (`mailto:` to `REPORT_EMAIL`,
  `lokalomat@aitransformation.pl`) with the answer, the model and where it ran.
  Nothing is sent until the user presses Send. The phone app gets
  `tauri-plugin-opener` with permission for `mailto:` URLs only.

## Consequences

- The release build runs R8 minification for the first time. The on-phone chat
  (LiteRT-LM plugin, Kotlin) must be tested on a release build before upload;
  missing keep rules would show up there. The first release build crashed at
  startup: R8 full mode (AGP 9) drops the default constructor kept by bare
  `-keep class` rules of Room, ML Kit/Firebase and WorkManager. `proguard-rules.pro`
  now keeps those constructors explicitly.
- In the release build, plain HTTP (`usesCleartextTraffic`) is off. The phone talks
  to the computer from Rust (not through the Android network stack), so this
  should not matter. Check it in the same test.
- Still needed outside the code: the Play developer account (organisation, D-U-N-S),
  the privacy policy URL, the Data safety form and the store listing.
