package com.furkanyildirim.mailhere

import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import com.furkanyildirim.mailhere.plugin.ArkaPlan

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  /**
   * "Anlık bildirim" açıksa ön plan hizmeti uygulama ekrana gelince başlar
   * (Android 12+ arka plandan başlatmaya izin vermez; telefon yeniden
   * başlayınca da ilk açılışta kurulur).
   */
  override fun onResume() {
    super.onResume()
    ArkaPlan.baslatGerekirse(this)
  }

  /**
   * Kökte geri: Android 12+ etkinliği kapatmaz, görevi arka plana alır. Eski
   * sürümlerde de aynısı yapılır; kapatılan etkinlik Tauri'de süreci (ve
   * anlık bildirim bağlantısını) sonlandırırdı. Arayüzün geri yığını
   * (çekmece, alt sayfa, okuma) önce kendi geri işleyicisinde boşalır.
   */
  @Suppress("OVERRIDE_DEPRECATION", "DEPRECATION")
  override fun onBackPressed() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S && isTaskRoot &&
      !onBackPressedDispatcher.hasEnabledCallbacks()
    ) {
      moveTaskToBack(true)
      return
    }
    super.onBackPressed()
  }
}
