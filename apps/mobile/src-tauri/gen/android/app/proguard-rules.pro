# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# R8 full mode (AGP 9) no longer keeps the default constructor for a bare
# "-keep class X" rule. These libraries create the classes by reflection and
# ship such bare rules, so the release build crashed at startup (WorkManager's
# Room database, ML Kit's component registrars). See ADR 0040.
-keep class * extends androidx.room.RoomDatabase { <init>(); }
-keep class * implements com.google.firebase.components.ComponentRegistrar { <init>(); }
-keep class * extends androidx.work.Worker { <init>(...); }
-keep class * extends androidx.work.InputMerger { <init>(); }
-keep class * extends androidx.startup.Initializer { <init>(); }
-keep class * implements androidx.versionedparcelable.VersionedParcelable { <init>(); }
