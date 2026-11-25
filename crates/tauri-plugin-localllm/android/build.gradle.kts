import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "pl.lokalsi.localllm"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    // Gemma 4 / Qwen3.5 .litertlm files need a recent runtime; 0.17+ is built with Kotlin metadata
    // 2.4, so the project uses Kotlin 2.4.x (apps/mobile gen/android/build.gradle.kts).
    implementation("com.google.ai.edge.litertlm:litertlm-android:0.20.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("androidx.core:core-ktx:1.9.0")
    implementation(project(":tauri-android"))
}
