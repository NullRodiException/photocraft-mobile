// Packages the Rust library (built by `cargo xtask android` into target/android/jniLibs) with
// AndroidX GameActivity. There is no Java or Kotlin source of our own.
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
        versionCode = 1
        versionName = "0.1.0"
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
