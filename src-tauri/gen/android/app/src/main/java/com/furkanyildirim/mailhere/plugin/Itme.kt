package com.furkanyildirim.mailhere.plugin

import android.content.Context
import android.os.PowerManager
import android.util.Log
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Sunucu itmesi (push), Android tarafı. Ayrıntı: `mailhere/docs/PUSH.md`.
 *
 * Firebase istemci ayarları uygulamaya gömülmez: Mailhere bildirim
 * aktarıcısından (`/v1/yapilandirma`) alınır ve tercih dosyasında saklanır;
 * Firebase ilk kez o ayarla başlatılır. Böylece APK'da ve depoda Firebase
 * anahtarı yoktur, aktarıcı yokken uygulama itmesiz (ön plan hizmeti ve
 * yoklama işiyle) her zamanki gibi çalışır.
 *
 * Firebase kayıt jetonu kasaya `push-jeton` adıyla yazılır; Rust çekirdeği
 * (`mh_app::push`) jetonla aktarıcıda abone açar ve posta sunucusunda
 * aboneliği kurar. Jeton yenilenince çekirdeğe haber verilir.
 */
object Itme {
    /** `mh_app::push::RELAY_URL` ile aynı. */
    private const val AKTARICI = "https://mailhere-push.arnexlab.com"
    private const val TERCIH = "mailhere-itme"
    private const val AYAR = "firebase"
    private const val JETON = "push-jeton"
    private const val TAG = "Mailhere"

    private fun tercih(ctx: Context) =
        ctx.applicationContext.getSharedPreferences(TERCIH, Context.MODE_PRIVATE)

    private fun secenekler(json: String?): FirebaseOptions? {
        val o = try {
            JSONObject(json ?: return null)
        } catch (e: Exception) {
            return null
        }
        val uygulama = o.optString("appId").takeIf { it.isNotBlank() } ?: return null
        val anahtar = o.optString("apiKey").takeIf { it.isNotBlank() } ?: return null
        val proje = o.optString("projectId").takeIf { it.isNotBlank() } ?: return null
        val gonderen = o.optString("senderId").takeIf { it.isNotBlank() } ?: return null
        return FirebaseOptions.Builder()
            .setApplicationId(uygulama)
            .setApiKey(anahtar)
            .setProjectId(proje)
            .setGcmSenderId(gonderen)
            .build()
    }

    /** Kayıtlı ayarla Firebase'i başlatır; hazırsa `true`. */
    private fun baslat(ctx: Context): Boolean {
        if (FirebaseApp.getApps(ctx).isNotEmpty()) return true
        val s = secenekler(tercih(ctx).getString(AYAR, null)) ?: return false
        return try {
            FirebaseApp.initializeApp(ctx.applicationContext, s)
            true
        } catch (e: Exception) {
            Log.w(TAG, "Firebase başlatılamadı: ${e.javaClass.simpleName}")
            false
        }
    }

    /**
     * Süreç başında (`MailhereApp.onCreate`): kayıtlı ayarla Firebase'i açar,
     * arka planda aktarıcıdan güncel ayarı alır (ilk kurulumda ya da ayar
     * değiştiyse kaydeder) ve kayıt jetonunu kasaya yazar.
     */
    fun kur(ctx: Context) {
        val app = ctx.applicationContext
        val hazir = baslat(app)
        if (hazir) jetonIste(app)
        Thread {
            try {
                val yeni = ayarAl() ?: return@Thread
                if (secenekler(yeni) == null) return@Thread
                val eski = tercih(app).getString(AYAR, null)
                if (eski != yeni) tercih(app).edit().putString(AYAR, yeni).apply()
                if (!hazir && baslat(app)) jetonIste(app)
            } catch (e: Exception) {
                Log.i(TAG, "itme ayarı alınamadı: ${e.javaClass.simpleName}")
            }
        }.start()
    }

    /** Aktarıcının Firebase istemci ayarı (gizli değildir) ya da `null`. */
    private fun ayarAl(): String? {
        val baglanti = URL("$AKTARICI/v1/yapilandirma").openConnection() as HttpURLConnection
        return try {
            baglanti.connectTimeout = 10_000
            baglanti.readTimeout = 10_000
            if (baglanti.responseCode != 200) return null
            val govde = baglanti.inputStream.bufferedReader().use { it.readText() }
            JSONObject(govde).optJSONObject("firebase")?.toString()
        } finally {
            baglanti.disconnect()
        }
    }

    private fun jetonIste(ctx: Context) {
        try {
            FirebaseMessaging.getInstance().token
                .addOnSuccessListener { jeton -> jetonKaydet(ctx, jeton) }
                .addOnFailureListener { e ->
                    Log.i(TAG, "Firebase jetonu alınamadı: ${e.javaClass.simpleName}")
                }
        } catch (e: Exception) {
            Log.w(TAG, "Firebase jetonu istenemedi: ${e.javaClass.simpleName}")
        }
    }

    /** Jeton değiştiyse kasaya yazar ve çalışan çekirdeğe haber verir. */
    fun jetonKaydet(ctx: Context, jeton: String) {
        if (jeton.isBlank()) return
        try {
            if (ArkaKopru.kasaOku(ctx, JETON) == jeton) return
            ArkaKopru.kasaYaz(ctx, JETON, jeton)
        } catch (e: Exception) {
            Log.w(TAG, "itme jetonu kasaya yazılamadı: ${e.javaClass.simpleName}")
            return
        }
        if (ArkaKopru.cekirdekVar()) ArkaKopru.pushGonder("{\"t\":\"jeton\"}")
    }
}

/**
 * Firebase iletisi (aktarıcıdan, yüksek öncelikli veri iletisi). İleti
 * posta içeriği taşımaz: `t` = `degisti` (eşitle) ya da `dogrula`
 * (sunucunun abonelik doğrulama kodu).
 *
 * - Çekirdek çalışıyorsa ileti ona verilir (Rust eşitlemeyi ya da
 *   doğrulamayı başlatır); işlemci 30 sn uyanık tutulur.
 * - Çekirdek yoksa "değişti" için hemen çalışan bir iş zamanlanır
 *   ([ArkaPlanIsi.hemen]: hizmeti ya da tek seferlik eşitle-bildir'i açar).
 *   Doğrulama kodu kasaya yazılır; çekirdek açılınca uygulanır.
 */
class ItmeServisi : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        Itme.jetonKaydet(applicationContext, token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        val veri = JSONObject(message.data as Map<*, *>)
        val tur = veri.optString("t")
        if (tur != "degisti" && tur != "dogrula") return
        val kilit = getSystemService(PowerManager::class.java)
            ?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Mailhere:itme")
        kilit?.setReferenceCounted(false)
        kilit?.acquire(UYANIK_MS)
        if (ArkaKopru.cekirdekVar() && ArkaKopru.pushGonder(veri.toString())) return
        kilit?.release()
        when (tur) {
            "degisti" -> ArkaPlanIsi.hemen(applicationContext)
            "dogrula" -> try {
                ArkaKopru.kasaYaz(applicationContext, BEKLEYEN, veri.toString())
                ArkaPlanIsi.hemen(applicationContext)
            } catch (e: Exception) {
                Log.w(TAG, "doğrulama kodu saklanamadı: ${e.javaClass.simpleName}")
            }
        }
    }

    companion object {
        private const val TAG = "Mailhere"
        /** Eşitleme ve bildirim için uyanıklık üst sınırı. */
        private const val UYANIK_MS = 30_000L
        /** `mh_app::push::PENDING_KEY` ile aynı. */
        const val BEKLEYEN = "push-bekleyen"
    }
}
