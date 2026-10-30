// The Fire TV APK. Native libraries, SDL's Java sources and the assets are built or staged outside
// Gradle (the `firetv` job in .github/workflows/ci.yml) and passed in as directories:
//   gradle assembleDebug -Pplx.sdlJava=<dir> -Pplx.jniLibs=<dir> -Pplx.assets=<dir>
// -Pplx.versionCode and -Pplx.versionName override the dev build's 900 and 0.10.0-dev.
// Signing reads FIRETV_{DEBUG,RELEASE}_KEYSTORE (a file path), _KEYSTORE_PASSWORD, _KEY_ALIAS and
// _KEY_PASSWORD from the environment. Unset, debug falls back to AGP's own debug key and release is
// unsigned; ci/check-apk.sh fails both.
plugins {
    id("com.android.application") version "8.13.2"
}

fun dirProperty(name: String): String =
    providers.gradleProperty(name).orNull ?: error("pass -P$name=<directory>")

fun env(name: String): String? = providers.environmentVariable(name).orNull

android {
    namespace = "com.beb.plxnative"
    compileSdk = 35
    ndkVersion = "27.3.13750724"

    defaultConfig {
        applicationId = "com.beb.plxnative"
        minSdk = 25
        targetSdk = 28
        versionCode = providers.gradleProperty("plx.versionCode").orNull?.toInt() ?: 900
        versionName = providers.gradleProperty("plx.versionName").orNull ?: "0.10.0-dev"
    }

    signingConfigs {
        for (type in listOf("debug", "release")) {
            val prefix = "FIRETV_${type.uppercase()}_"
            val store = env(prefix + "KEYSTORE") ?: continue
            maybeCreate(type).apply {
                storeFile = file(store)
                storePassword = env(prefix + "KEYSTORE_PASSWORD")
                keyAlias = env(prefix + "KEY_ALIAS")
                keyPassword = env(prefix + "KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        getByName("debug") {
            applicationIdSuffix = ".dev"
        }
        getByName("release") {
            signingConfig = signingConfigs.findByName("release")
        }
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
