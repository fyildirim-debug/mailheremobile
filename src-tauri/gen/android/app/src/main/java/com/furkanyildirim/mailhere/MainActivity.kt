package com.furkanyildirim.mailhere

import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.ViewTreeObserver
import android.view.animation.PathInterpolator
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.annotation.RequiresApi
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import com.furkanyildirim.mailhere.plugin.ArkaPlan
import com.furkanyildirim.mailhere.plugin.Yasam

class MainActivity : TauriActivity() {
  /** İlk arayüz ekranı hazır (ya da bekleme sınırı doldu): açılış ekranı kalkar. */
  private var acilisHazir = false
  private val acilisSayaci = Handler(Looper.getMainLooper())

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
    // Yalnız soğuk açılışta: yeniden kurulan etkinlikte arayüz sinyali yeniden gelmez.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && savedInstanceState == null) acilisiTut()
  }

  /**
   * Android 12+ açılış ekranı (tema: küçük işaret, zemin rengi) ilk arayüz
   * ekranı DOM'a yazılana dek kalır; böylece işaret → boş zemin → arayüz
   * sıçraması olmaz. İçerik görünümünün çizimi ertelenir (WebView yine yüklenir
   * ve betik çalışır); sinyal `mhAcilis` kanalından gelir ya da en çok
   * [ACILIS_EN_COK_MS] beklenir. Çıkışta işaret 200 ms'de solar.
   */
  @RequiresApi(Build.VERSION_CODES.S)
  private fun acilisiTut() {
    val icerik = findViewById<View>(android.R.id.content)
    icerik.viewTreeObserver.addOnPreDrawListener(object : ViewTreeObserver.OnPreDrawListener {
      override fun onPreDraw(): Boolean {
        if (!acilisHazir) return false
        icerik.viewTreeObserver.removeOnPreDrawListener(this)
        return true
      }
    })
    acilisSayaci.postDelayed({ acilisBitti() }, ACILIS_EN_COK_MS)
    splashScreen.setOnExitAnimationListener { gorunum ->
      gorunum.animate()
        .alpha(0f)
        .setDuration(ACILIS_SOLMA_MS)
        .setInterpolator(PathInterpolator(0.16f, 1f, 0.3f, 1f))
        .withEndAction { gorunum.remove() }
        .start()
    }
  }

  private fun acilisBitti() {
    if (acilisHazir) return
    acilisHazir = true
    acilisSayaci.removeCallbacksAndMessages(null)
  }

  /**
   * Hazır sinyali kanalı (`app/src/lib/ready.ts`). Yalnız uygulamanın kendi
   * kökeninden ve ana çerçeveden gelen `hazir` iletisi kabul edilir; kanal
   * başka hiçbir şey yapmaz ve veri taşımaz. Sayfa yüklenmeden önce eklenir
   * (wry `setWebView` → `loadUrl` sırası).
   */
  override fun onWebViewCreate(webView: WebView) {
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
    WebViewCompat.addWebMessageListener(webView, "mhAcilis", ACILIS_KOKENLERI) { _, ileti, _, anaCerceve, _ ->
      if (anaCerceve && ileti.data == "hazir") runOnUiThread { acilisBitti() }
    }
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
    acilisSayaci.removeCallbacksAndMessages(null)
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

  private companion object {
    /** Arayüz yanıt vermezse açılış ekranı en çok bu kadar kalır. */
    const val ACILIS_EN_COK_MS = 2500L
    const val ACILIS_SOLMA_MS = 200L
    /** Tauri'nin Android'deki uygulama kökeni (`useHttpsScheme` kapalı; açık olursa https). */
    val ACILIS_KOKENLERI = setOf("http://tauri.localhost", "https://tauri.localhost")
  }
}
