package com.furkanyildirim.mailhere

import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import com.furkanyildirim.mailhere.plugin.ArkaPlan
import com.furkanyildirim.mailhere.plugin.Yasam

class MainActivity : TauriActivity() {
  /**
   * Bu süreçte ilk etkinlik daha önce yok edildiyse (anlık bildirim açıkken
   * son uygulamalardan kaydırma; Android'in arka planda etkinliği yok etmesi)
   * Tauri aynı süreçte yeni webview kuramaz: süreç [YenidenBaslatActivity]
   * ile temiz başlatılır, dokunulan bildirimin niyeti korunur. `super`
   * çağrılır (Android zorunlu kılar; tao bilinmeyen etkinliği yok sayar) ve
   * etkinlik hemen kapanır.
   */
  override fun onCreate(savedInstanceState: Bundle?) {
    if (Yasam.yokEdildi) {
      super.onCreate(savedInstanceState)
      YenidenBaslatActivity.tetikle(this)
      finish()
      return
    }
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    Yasam.etkinlikVar = true
  }

  /**
   * "Anlık bildirim" açıksa (varsayılan) ön plan hizmeti uygulama ekrana
   * gelince başlar ya da sürer (Android 12+ arka plandan başlatmaya yalnız
   * açılış, bildirim düğmesi gibi yollarla izin verir; burada ekrandayız).
   */
  override fun onResume() {
    super.onResume()
    Yasam.etkinlikVar = true
    ArkaPlan.baslatGerekirse(this)
  }

  /** Yapılandırma değişikliği (döndürme) yok etme sayılmaz: tao pencereyi korur. */
  override fun onDestroy() {
    super.onDestroy()
    if (!isChangingConfigurations) {
      Yasam.etkinlikVar = false
      Yasam.yokEdildi = true
    }
  }

  /**
   * Kökte geri: Android 12+ etkinliği kapatmaz, görevi arka plana alır. Eski
   * sürümlerde de aynısı yapılır; kapatılan etkinlik anlık bildirim
   * kapalıyken Tauri'de süreci sonlandırır. Arayüzün geri yığını
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
