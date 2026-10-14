//! `cargo xtask android [--abi arm64-v8a,x86_64] [--install] [--run] [--logcat]`
//!
//! Builds `apps/android` with cargo-ndk into `target/android/jniLibs/<abi>/`, then packages it
//! with AndroidX GameActivity through the Gradle project in `apps/android` (`gradlew
//! assembleRelease`, signed with the debug key). Output: `target/android/photocraft-mobile.apk`.
//!
//! Finds the SDK in `ANDROID_HOME`, `ANDROID_SDK_ROOT` or `%LOCALAPPDATA%\Android\Sdk`, and uses
//! its newest NDK. Gradle runs on `JAVA_HOME` (JDK 17+).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PACKAGE: &str = "dev.photocraftmobile.app";
const ACTIVITY: &str = "com.google.androidgamesdk.GameActivity";
const LIB: &str = "libphotocraft_mobile.so";
/// Must match `minSdk` in apps/android/build.gradle.kts.
const MIN_SDK: &str = "26";
const PROFILE: &str = "android-release";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("android") => android(args.get(1..).unwrap_or_default()),
        _ => Err("usage: cargo xtask android [--abi arm64-v8a,x86_64] [--install] [--run] [--logcat]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(Path::to_path_buf).ok_or_else(|| "no workspace root".into())
}

fn sdk_dir() -> Result<PathBuf, String> {
    ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))
        .chain(std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Android").join("Sdk")))
        .find(|p| p.is_dir())
        .ok_or_else(|| "Android SDK not found: set ANDROID_HOME".into())
}

/// The entry of `dir` that sorts last by numeric version (`28.2.13676358`).
fn newest(dir: &Path, accept: impl Fn(&Path) -> bool) -> Result<PathBuf, String> {
    let key = |p: &Path| -> Vec<u64> {
        p.file_name().map(|n| n.to_string_lossy().split(|c: char| !c.is_ascii_digit()).filter_map(|s| s.parse().ok()).collect()).unwrap_or_default()
    };
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| accept(p)).max_by_key(|p| key(p)).ok_or_else(|| format!("nothing usable in {}", dir.display()))
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{cmd:?}");
    eprintln!("> {shown}");
    let status = cmd.status().map_err(|e| format!("couldn't start {shown}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("failed ({status}): {shown}")) }
}

fn abi_triple(abi: &str) -> Result<&'static str, String> {
    Ok(match abi {
        "arm64-v8a" => "aarch64-linux-android",
        "x86_64" => "x86_64-linux-android",
        "armeabi-v7a" => "armv7-linux-androideabi",
        _ => {
            return Err(format!("unknown ABI {abi} (arm64-v8a, x86_64, armeabi-v7a)"));
        }
    })
}

fn android(args: &[String]) -> Result<(), String> {
    let flag = |f: &str| args.iter().any(|a| a == f);
    let abis: Vec<String> = args
        .iter()
        .position(|a| a == "--abi")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_else(|| vec!["arm64-v8a".into(), "x86_64".into()]);

    let root = root()?;
    let sdk = sdk_dir()?;
    let ndk = match std::env::var_os("ANDROID_NDK_HOME").filter(|v| !v.is_empty()) {
        Some(n) => PathBuf::from(n),
        None => newest(&sdk.join("ndk"), |p| p.join("source.properties").is_file())?,
    };
    let adb = sdk.join("platform-tools").join(exe("adb"));
    let out = root.join("target").join("android");
    let jni_libs = out.join("jniLibs");
    let gradle_dir = root.join("apps").join("android");
    eprintln!("SDK {}\nNDK {}", sdk.display(), ndk.display());

    // 1. The native library for each ABI, where build.gradle.kts picks it up.
    let mut build = Command::new("cargo");
    build.current_dir(&root).env("ANDROID_HOME", &sdk).env("ANDROID_NDK_HOME", &ndk).args(["ndk", "--platform", MIN_SDK]);
    for abi in &abis {
        build.args(["-t", abi]);
    }
    build.args(["build", "--profile", PROFILE, "-p", "photocraft-mobile-android"]);
    run(&mut build)?;
    let _ = std::fs::remove_dir_all(&jni_libs);
    for abi in &abis {
        let built = root.join("target").join(abi_triple(abi)?).join(PROFILE).join(LIB);
        let dest = jni_libs.join(abi);
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        std::fs::copy(&built, dest.join(LIB)).map_err(|e| format!("{}: {e}", built.display()))?;
    }

    // 2. The APK: GameActivity + the libraries.
    let gradlew = gradle_dir.join(if cfg!(windows) { "gradlew.bat" } else { "gradlew" });
    run(Command::new(&gradlew).current_dir(&gradle_dir).env("ANDROID_HOME", &sdk).args(["assembleRelease", "--console=plain", "--no-daemon"]))?;
    let built_apk = newest(&gradle_dir.join("build").join("outputs").join("apk").join("release"), |p| p.extension().is_some_and(|e| e == "apk"))?;
    let apk = out.join("photocraft-mobile.apk");
    std::fs::copy(&built_apk, &apk).map_err(|e| format!("{}: {e}", built_apk.display()))?;
    let size = std::fs::metadata(&apk).map(|m| m.len()).unwrap_or(0);
    eprintln!("APK: {} ({:.1} MB)", apk.display(), size as f64 / 1_048_576.0);

    if flag("--install") || flag("--run") {
        run(Command::new(&adb).args(["install", "-r"]).arg(&apk))?;
    }
    if flag("--run") {
        run(Command::new(&adb).args(["shell", "am", "start", "-n", &format!("{PACKAGE}/{ACTIVITY}")]))?;
    }
    if flag("--logcat") {
        run(Command::new(&adb).args(["logcat", "-s", "photocraft-mobile:*", "RustStdoutStderr:*", "AndroidRuntime:E", "DEBUG:*"]))?;
    }
    Ok(())
}
