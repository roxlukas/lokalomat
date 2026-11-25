plugins {
    `kotlin-dsl`
}

gradlePlugin {
    plugins {
        create("pluginsForCoolKids") {
            id = "rust"
            implementationClass = "RustPlugin"
        }
    }
}

repositories {
    google()
    mavenCentral()
}

dependencies {
    compileOnly(gradleApi())
    implementation("com.android.tools.build:gradle:9.3.1")
    // The on-phone model library (LiteRT-LM) is built with Kotlin 2.4 metadata.
    implementation("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.0")
}

