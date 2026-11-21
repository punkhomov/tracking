plugins {
    id("com.android.application") version "8.13.2"
    id("org.jetbrains.kotlin.android") version "2.2.21"
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.21"
}

val repository = rootProject.layout.projectDirectory.dir("../..")
val generated = layout.buildDirectory.dir("generated/tracking")
val nativeLibraries = generated.map { it.dir("jniLibs") }
val kotlinBindings = generated.map { it.dir("kotlin") }
val androidAbis = listOf("arm64-v8a", "x86_64")
val androidMinSdk = 26

android {
    namespace = "dev.punkhomov.tracking"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "dev.punkhomov.tracking"
        minSdk = androidMinSdk
        targetSdk = 36
        versionCode = providers.environmentVariable("TRACKING_ANDROID_VERSION_CODE").orNull?.toInt() ?: 1
        versionName = providers.environmentVariable("TRACKING_ANDROID_VERSION_NAME").orElse("0.2.0").get()
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk.abiFilters += androidAbis
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets.named("main") {
        java.srcDir(kotlinBindings)
        jniLibs.srcDir(nativeLibraries)
    }
}

kotlin {
    jvmToolchain(17)
}

val rustSources = fileTree(repository) {
    include("Cargo.toml", "Cargo.lock", ".cargo/config.toml", "rust-toolchain.toml")
    include("apps/android/native/**", "crates/**")
}
val ndkDirectory = androidComponents.sdkComponents.ndkDirectory

val buildRust by tasks.registering(Exec::class) {
    workingDir(repository)
    inputs.files(rustSources)
    inputs.property("androidAbis", androidAbis)
    inputs.property("androidMinSdk", androidMinSdk)
    inputs.property("ndkVersion", android.ndkVersion.orEmpty())
    outputs.dir(nativeLibraries)

    val targets = androidAbis.flatMap { listOf("-t", it) }
    commandLine(
        listOf("cargo", "ndk") + targets + listOf(
            "--platform", androidMinSdk.toString(),
            "-o", nativeLibraries.get().asFile.absolutePath,
            "build", "--release", "--locked", "-p", "tracking-android", "--lib",
        ),
    )
    doFirst {
        environment("ANDROID_NDK_HOME", ndkDirectory.get().asFile.absolutePath)
    }
}

val generateBindings by tasks.registering(Exec::class) {
    dependsOn(buildRust)
    workingDir(repository)
    val library = nativeLibraries.map { it.file("x86_64/libtracking_android.so") }
    inputs.files(rustSources)
    inputs.file(library)
    outputs.dir(kotlinBindings)
    commandLine(
        "cargo", "run", "--release", "--locked", "-p", "tracking-android",
        "--features", "bindgen", "--bin", "tracking-bindgen", "--",
        "generate", "--metadata-no-deps", "--no-format", "--language", "kotlin",
        "--out-dir", kotlinBindings.get().asFile.absolutePath, library.get().asFile.absolutePath,
    )
}

tasks.named("preBuild") {
    dependsOn(generateBindings)
}

dependencies {
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.9.4")
    implementation(platform("androidx.compose:compose-bom:2025.10.01"))
    implementation("androidx.compose.material3:material3")
    implementation("net.java.dev.jna:jna:5.18.1@aar")

    androidTestImplementation(platform("androidx.compose:compose-bom:2025.10.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
}
