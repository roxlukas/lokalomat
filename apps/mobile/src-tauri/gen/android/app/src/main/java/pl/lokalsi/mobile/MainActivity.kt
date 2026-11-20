package pl.lokalsi.mobile

import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    askForLocalNetwork()
  }

  /** Android 17 (API 37) blocks connections to the local network unless the
   *  user allows "devices on the local network"; the app only talks to the
   *  paired computer on the LAN, so ask right away. */
  private fun askForLocalNetwork() {
    if (Build.VERSION.SDK_INT < 37) return
    val permission = "android.permission.ACCESS_LOCAL_NETWORK"
    if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(arrayOf(permission), LOCAL_NETWORK_REQUEST)
    }
  }

  private companion object {
    const val LOCAL_NETWORK_REQUEST = 4711
  }
}
