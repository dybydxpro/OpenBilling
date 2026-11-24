# OpenBilling mobile (Android and iOS)

The phone and tablet app runs the **same UI and business logic** as the
desktop app (`../ui`, `../core`). This crate only contains the platform entry
points and the packaging files.

| File | Purpose |
|------|---------|
| `src/lib.rs` | `android_main` (Android entry point) and `run()` shared by all platforms |
| `src/main.rs` | iOS entry point. On Windows, macOS or Linux it opens a phone-sized preview |
| `android/res` | Android launcher icons |
| `ios/` | XcodeGen project, `Info.plist` and app icon for iOS |

## How mobile differs from desktop

- **Files:** phones have no save/open dialogs that a Rust app can use without
  Java or Swift glue code. Instead, invoices, CSV reports, template exports and
  backups are written to an **OpenBilling folder**. Importing a template or
  restoring a backup shows an in-app list of the matching files in that folder.

  | Platform | Database (private) | OpenBilling folder (you can reach it) |
  |----------|--------------------|----------------------------------------|
  | Android  | app internal storage | `Android/data/com.openbilling.app/files/OpenBilling` (open it with a USB connection or `adb pull`) |
  | iOS      | `Library/Application Support/OpenBilling` | Files app → On My iPhone → OpenBilling |

- **Navigation:** on phone-width screens a bottom tab bar replaces the
  sidebar. It has five tabs (Home, Orders, Invoices, Clients, More), and More
  opens a sheet with Templates, Reports, Backup & settings and Lock.
- **Android Back button:** Back closes the open sheet or dialog first, then
  steps back a page (an order goes back to Orders, other pages go back to
  Home). Only Back on Home or on the sign-in screen leaves the app.
- **Layout:** the UI keeps clear of the status bar, notch and gesture bar
  (safe-area insets), and the content shrinks while the on-screen keyboard is
  open so the focused field stays reachable.
- **Clipboard:** "Copy key" uses Slint's own clipboard, which works on
  Android and iOS as well.

## Preview on a computer

```sh
cargo run -p app-mobile                          # phone-sized window, data in ./mobile-preview-data
OPENBILLING_DATA_DIR=./demo-data cargo run -p app-mobile
```

## Android

One-time setup:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-apk
# Android SDK with: platforms;android-35, build-tools, ndk;29.0.14206865
export ANDROID_HOME=$HOME/Android/Sdk                 # Windows: %LOCALAPPDATA%\Android\Sdk
export ANDROID_NDK_ROOT=$ANDROID_HOME/ndk/29.0.14206865
```

NDK 29 supports API levels up to 35, so the app targets API 35. `cargo-apk`
ignores SDK platform folders it can't parse, such as `android-36.1`.

Build and install (run these in `mobile/`):

```sh
cargo apk run --lib --target x86_64-linux-android     # emulator
cargo apk run --lib --target aarch64-linux-android    # phone over USB
cargo apk build --lib --release                       # APKs for both ABIs in target/release/apk/
```

Release builds need a signing key. Pass it through environment variables, so
no key path or password ends up in the repository:

```sh
export CARGO_APK_RELEASE_KEYSTORE=/path/to/release.keystore
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=...
cargo apk build --lib --release --target aarch64-linux-android
```

With no `--target`, `cargo apk build` packages every target in
`build_targets` (arm64-v8a and x86_64) into one APK. 32-bit ARM (armeabi-v7a)
isn't supported: Slint's Skia renderer has no prebuilt binaries for it, and
Google Play requires 64-bit builds anyway.

Debug builds use `~/.android/debug.keystore`. Debug APKs are large (about
440 MB) because they keep full debug symbols. Release builds are stripped.

If the app closes on start, read
`adb shell run-as com.openbilling.app cat files/crash.log`.

## iOS (requires a Mac with Xcode)

Not built or tested yet, because this needs macOS.

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
brew install xcodegen
cd mobile/ios && xcodegen generate && open OpenBilling.xcodeproj
```

Select your team under *Signing & Capabilities*, then run on a simulator or a
device. A post-compile script builds `openbilling-mobile` for the right Rust
target and uses it as the app executable.

## Next steps

- Share sheet and system file picker (Android Storage Access Framework,
  `UIDocumentPickerViewController`) so files can go straight to email, Drive
  or iCloud. This needs a small amount of Java or Swift glue.
- Open PDFs in the system viewer from inside the app.
