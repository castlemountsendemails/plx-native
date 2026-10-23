// The Fire TV APK. Native libraries, SDL's Java sources and the assets are built or staged outside
// Gradle (the `firetv` job in .github/workflows/ci.yml) and passed in as directories:
//   gradle assembleDebug -Pplx.sdlJava=<dir> -Pplx.jniLibs=<dir> -Pplx.assets=<dir>
plugins {
    id("com.android.application") version "8.13.2"
}

fun dirProperty(name: String): String =
    providers.gradleProperty(name).orNull ?: error("pass -P$name=<directory>")

android {
    namespace = "com.beb.plxnative"
    compileSdk = 35
    ndkVersion = "27.3.13750724"

    defaultConfig {
        applicationId = "com.beb.plxnative.debug"
        minSdk = 25
        targetSdk = 28
    }

    sourceSets["main"].apply {
        java.srcDir(dirProperty("plx.sdlJava"))
        jniLibs.srcDir(dirProperty("plx.jniLibs"))
        assets.srcDir(dirProperty("plx.assets"))
    }

    // Compressed in the APK and extracted at install, which the manifest reports as extractNativeLibs="true".
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    // targetSdk 28 is fixed by the oldest stick; lint's expired-target check must not fail the build.
    lint {
        checkReleaseBuilds = false
        abortOnError = false
    }
}
