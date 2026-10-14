// Packages the Rust library (built by `cargo xtask android` into target/android/jniLibs) with
// AndroidX GameActivity, through MainActivity.java (immersive full screen, palm rejection).
plugins {
    id("com.android.application") version "8.7.3"
}

android {
    namespace = "dev.photocraftmobile.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.photocraftmobile.app"
        minSdk = 26
        targetSdk = 35
        // `cargo xtask android` bumps the build number in `build-number` and passes it here, so
        // each build is a new version that installs over the last one.
        val build = (project.findProperty("buildNumber") as String?)?.toIntOrNull() ?: 1
        versionCode = build
        versionName = "0.1.$build"
    }

    sourceSets["main"].jniLibs.srcDirs("../../target/android/jniLibs")

    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            // A local build: signed with the debug key, so it installs with `adb install`.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // The version android-activity 0.6 is written against (its README).
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    // One Kotlin standard library: AndroidX pulls kotlin-stdlib 1.8 and the older -jdk7/-jdk8
    // artifacts 1.6, whose classes collide (checkReleaseDuplicateClasses).
    implementation(platform("org.jetbrains.kotlin:kotlin-bom:1.8.22"))
}
