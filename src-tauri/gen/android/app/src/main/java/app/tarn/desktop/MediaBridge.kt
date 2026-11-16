package app.tarn.desktop

/** JNI bridge to the Rust player (see src-tauri/src/android_media.rs). */
object MediaBridge {
  init {
    System.loadLibrary("tarn_lib")
  }

  @JvmStatic external fun nativeState(): String
  @JvmStatic external fun nativeAction(action: String)
  @JvmStatic external fun nativeLoginCookies(header: String)
  @JvmStatic external fun nativeLoginCancel()
}
