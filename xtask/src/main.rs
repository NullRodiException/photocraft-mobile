//! `cargo xtask android [--abi arm64-v8a,x86_64] [--install] [--run] [--logcat]`
//!
//! Builds `apps/android` with cargo-ndk into `target/android/jniLibs/<abi>/`, then packages it
//! with AndroidX GameActivity through the Gradle project in `apps/android` (`gradlew
//! assembleRelease`, signed with the debug key). Each build is a new version: the build number in
//! `apps/android/build-number` goes up by one and becomes the `versionCode` (`versionName`
//! `0.1.<n>`). Output: `target/android/photocraft-mobile-v<n>.apk`, or
//! `photocraft-mobile-v<n>-<abis>.apk` for a build without arm64-v8a (`--abi x86_64` for the
//! emulator); earlier APKs stay.
//!
//! Finds the SDK in `ANDROID_HOME`, `ANDROID_SDK_ROOT` or `%LOCALAPPDATA%\Android\Sdk`, and uses
//! its newest NDK. Gradle runs on `JAVA_HOME` (JDK 17+).
//!
//! `cargo xtask icons` renders PhotoCraft's app icon (`../photocraft/assets/app-icon/photocraft.svg`)
//! into the foreground layer of the Android adaptive icon, one PNG per screen density, under
//! `apps/android/src/main/res/mipmap-*/`. The PNGs are committed; rerun it when the icon changes.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PACKAGE: &str = "dev.photocraftmobile.app";
const ACTIVITY: &str = ".MainActivity";
const LIB: &str = "libphotocraft_mobile.so";
/// Must match `minSdk` in apps/android/build.gradle.kts.
const MIN_SDK: &str = "26";
const PROFILE: &str = "android-release";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("android") => android(args.get(1..).unwrap_or_default()),
        Some("icons") => icons(),
        _ => Err("usage: cargo xtask android [--abi arm64-v8a,x86_64] [--install] [--run] [--logcat]\n       cargo xtask icons".into()),
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

/// The last build number, in `apps/android/`; each `cargo xtask android` builds the next one.
const BUILD_NUMBER: &str = "build-number";

/// The build after the one recorded in the build-number file (`None` or garbage: the first, 1
/// being the version built before the file existed).
fn next_build_number(recorded: Option<&str>) -> u32 {
    recorded.and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(1).saturating_add(1)
}

/// `photocraft-mobile-v7.apk`. A build without arm64 (the emulator) says which ABIs it has, so it
/// is never mistaken for one that runs on phones and tablets.
fn apk_name(build: u32, abis: &[String]) -> String {
    if abis.iter().any(|a| a == "arm64-v8a") { format!("photocraft-mobile-v{build}.apk") } else { format!("photocraft-mobile-v{build}-{}.apk", abis.join("-")) }
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

    // 2. The APK: GameActivity + the libraries, as the next build number (versionCode).
    let number_file = gradle_dir.join(BUILD_NUMBER);
    let build_number = next_build_number(std::fs::read_to_string(&number_file).ok().as_deref());
    eprintln!("build {build_number} (version 0.1.{build_number})");
    let gradlew = gradle_dir.join(if cfg!(windows) { "gradlew.bat" } else { "gradlew" });
    run(Command::new(&gradlew).current_dir(&gradle_dir).env("ANDROID_HOME", &sdk).args([
        "assembleRelease",
        "--console=plain",
        "--no-daemon",
        &format!("-PbuildNumber={build_number}"),
    ]))?;
    let built_apk = newest(&gradle_dir.join("build").join("outputs").join("apk").join("release"), |p| p.extension().is_some_and(|e| e == "apk"))?;
    // Every build is a new file next to the earlier ones.
    let apk = out.join(apk_name(build_number, &abis));
    std::fs::copy(&built_apk, &apk).map_err(|e| format!("{}: {e}", built_apk.display()))?;
    std::fs::write(&number_file, format!("{build_number}\n")).map_err(|e| format!("{}: {e}", number_file.display()))?;
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

/// Adaptive icon geometry (dp): the 108 × 108 foreground layer, of which launchers show the
/// middle 72 × 72 through their mask (circle, squircle, rounded square).
const ICON_LAYER_DP: f32 = 108.0;
/// How much of the layer the PhotoCraft tile covers: inside the 66 dp safe zone, so a round mask
/// (Pixel, many Samsung themes) keeps the ears and tail tips; only the blue corners fall outside.
const ICON_TILE_DP: f32 = 64.0;
/// Android screen densities and their scale over mdpi.
const DENSITIES: [(&str, f32); 5] = [("mdpi", 1.0), ("hdpi", 1.5), ("xhdpi", 2.0), ("xxhdpi", 3.0), ("xxxhdpi", 4.0)];

fn icons() -> Result<(), String> {
    use resvg::{tiny_skia, usvg};
    let root = root()?;
    let source = root.join("..").join("photocraft").join("assets").join("app-icon").join("photocraft.svg");
    let svg = std::fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    // The master clips the tile to a rounded square; the launcher applies its own shape instead.
    let full_bleed = svg.replacen(r#" clip-path="url(#t)""#, "", 1);
    if full_bleed == svg {
        return Err(format!("{}: the rounded-square clip wasn't found; check the icon still has `clip-path=\"url(#t)\"`", source.display()));
    }
    let tree = usvg::Tree::from_str(&full_bleed, &usvg::Options::default()).map_err(|e| format!("{}: {e}", source.display()))?;
    let res = root.join("apps").join("android").join("src").join("main").join("res");
    for (density, scale) in DENSITIES {
        let layer = (ICON_LAYER_DP * scale).round();
        let tile = ICON_TILE_DP * scale;
        let mut pixmap = tiny_skia::Pixmap::new(layer as u32, layer as u32).ok_or("couldn't allocate the icon")?;
        let fit = tile / tree.size().width().max(tree.size().height());
        let offset = (layer - tile) / 2.0;
        resvg::render(&tree, tiny_skia::Transform::from_scale(fit, fit).post_translate(offset, offset), &mut pixmap.as_mut());
        let dir = res.join(format!("mipmap-{density}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let out = dir.join("ic_launcher_foreground.png");
        pixmap.save_png(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        eprintln!("{} ({layer} px)", out.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_build_takes_the_next_number() {
        assert_eq!(next_build_number(Some("3\n")), 4);
        assert_eq!(next_build_number(Some(" 41 ")), 42);
        assert_eq!(next_build_number(None), 2, "the first numbered build follows versionCode 1");
        assert_eq!(next_build_number(Some("garbage")), 2);
        assert_eq!(next_build_number(Some(&u32::MAX.to_string())), u32::MAX);
    }

    #[test]
    fn apks_are_named_by_build() {
        let abis = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(apk_name(4, &abis(&["arm64-v8a", "x86_64"])), "photocraft-mobile-v4.apk");
        assert_eq!(apk_name(5, &abis(&["x86_64"])), "photocraft-mobile-v5-x86_64.apk");
    }
}
