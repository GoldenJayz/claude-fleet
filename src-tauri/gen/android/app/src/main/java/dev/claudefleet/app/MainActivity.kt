package dev.claudefleet.app

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import java.io.File

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Dark app: light status/navigation bar icons over our own background.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)

    // Keep the page clear of the status bar, the gesture bar and the keyboard: the web view
    // shrinks instead of being drawn underneath them (so the terminal stays above the keyboard).
    val root = findViewById<View>(android.R.id.content)
    root.setBackgroundColor(Color.rgb(11, 11, 18))
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    webView.addJavascriptInterface(UpdateInstaller(this), "FleetAndroid")
  }
}

/**
 * Hands a downloaded update to Android's installer. The Rust side has already checked the APK
 * against the release's SHA-256; Android then only installs it over this app if it is signed
 * with the same key. Only files in this app's own cache/updates folder are accepted.
 */
class UpdateInstaller(private val activity: Activity) {
  @JavascriptInterface
  fun installApk(path: String): String {
    val dir = File(activity.cacheDir, "updates").canonicalFile
    val file = File(path).canonicalFile
    if (file.parentFile != dir || !file.isFile || !file.name.endsWith(".apk")) return "invalid"
    if (Build.VERSION.SDK_INT >= 26 && !activity.packageManager.canRequestPackageInstalls()) {
      // One-time: let the user allow Claude Fleet to install its updates.
      activity.runOnUiThread {
        activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + activity.packageName)))
      }
      return "permission"
    }
    val uri = FileProvider.getUriForFile(activity, activity.packageName + ".fileprovider", file)
    val intent = Intent(Intent.ACTION_VIEW)
      .setDataAndType(uri, "application/vnd.android.package-archive")
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    activity.runOnUiThread { activity.startActivity(intent) }
    return "ok"
  }
}
