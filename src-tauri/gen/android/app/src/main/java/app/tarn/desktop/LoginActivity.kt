package app.tarn.desktop

import android.app.Activity
import android.graphics.Bitmap
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import java.net.URI

/**
 * Google / YouTube Music sign-in in a plain WebView. Cookies are read from
 * Android's CookieManager (HttpOnly ones included) and handed to Rust once a
 * complete session exists. Values are never logged.
 */
class LoginActivity : Activity() {
  private lateinit var web: WebView
  private val handler = Handler(Looper.getMainLooper())
  private var done = false
  private var polling = false
  private var tries = 0

  private val poll = object : Runnable {
    override fun run() {
      if (done) return
      val header = collect()
      if (isComplete(header)) {
        done = true
        MediaBridge.nativeLoginCookies(header)
        finish()
        return
      }
      if (++tries < 180) handler.postDelayed(this, 700) else polling = false
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    val cm = CookieManager.getInstance()
    cm.setAcceptCookie(true)

    web = WebView(this)
    cm.setAcceptThirdPartyCookies(web, true)
    web.settings.javaScriptEnabled = true
    web.settings.domStorageEnabled = true
    // Google refuses sign-in inside an embedded WebView ("; wv" / Version/4.0).
    web.settings.userAgentString =
      "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) " +
        "Chrome/126.0.0.0 Mobile Safari/537.36"
    web.webViewClient = object : WebViewClient() {
      override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) = maybePoll(url)
      override fun onPageFinished(view: WebView?, url: String?) = maybePoll(url)
    }

    val root = FrameLayout(this)
    root.addView(web, ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
    // targetSdk 37 draws edge-to-edge: keep the page clear of status / nav bars.
    root.setOnApplyWindowInsetsListener { v, insets ->
      @Suppress("DEPRECATION")
      v.setPadding(
        insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
        insets.systemWindowInsetRight, insets.systemWindowInsetBottom
      )
      insets
    }
    setContentView(root)
    web.loadUrl(LOGIN_URL)
  }

  private fun maybePoll(url: String?) {
    val host = try { URI(url ?: "").host ?: "" } catch (_: Throwable) { "" }
    if (!polling && (host == "youtube.com" || host.endsWith(".youtube.com"))) {
      polling = true
      handler.postDelayed(poll, 700)
    }
  }

  private fun collect(): String {
    val cm = CookieManager.getInstance()
    val merged = LinkedHashMap<String, String>()
    for (u in listOf(
      "https://music.youtube.com", "https://www.youtube.com",
      "https://accounts.google.com", "https://www.google.com"
    )) {
      cm.getCookie(u)?.split(";")?.forEach { kv ->
        val i = kv.indexOf('=')
        if (i > 0) {
          val k = kv.substring(0, i).trim()
          if (k.isNotEmpty() && !merged.containsKey(k)) merged[k] = kv.substring(i + 1).trim()
        }
      }
    }
    return merged.entries.joinToString("; ") { it.key + "=" + it.value }
  }

  private fun isComplete(h: String): Boolean {
    val sapi = h.contains("SAPISID=") || h.contains("__Secure-3PAPISID=") || h.contains("__Secure-1PAPISID=")
    val sid = h.contains("__Secure-1PSID=") || h.contains("__Secure-3PSID=") ||
      h.contains("SID=") || h.contains("LOGIN_INFO=")
    return sapi && sid
  }

  @Deprecated("Deprecated in Java")
  override fun onBackPressed() {
    if (web.canGoBack()) web.goBack() else finish()
  }

  override fun onDestroy() {
    handler.removeCallbacksAndMessages(null)
    if (!done) {
      done = true
      MediaBridge.nativeLoginCancel()
    }
    web.destroy()
    super.onDestroy()
  }

  companion object {
    const val LOGIN_URL =
      "https://accounts.google.com/ServiceLogin?service=youtube&passive=true" +
        "&continue=https%3A%2F%2Fmusic.youtube.com%2F&followup=https%3A%2F%2Fmusic.youtube.com%2F&hl=en"
  }
}
