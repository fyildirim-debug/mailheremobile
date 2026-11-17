package com.furkanyildirim.mailhere.plugin

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import android.widget.Toast
import androidx.core.app.RemoteInput
import com.furkanyildirim.mailhere.R
import org.json.JSONArray
import org.json.JSONObject

/**
 * Bildirim düğmeleri (Kodu kopyala, Yanıtla, Okundu, Arşivle, Sil, İstenmeyen,
 * Geri al), kaydırıp kapatma, anlık bildirimin "Kapat" düğmesi ve uyanma alarmı. Dışa
 * kapalıdır (`exported=false`); niyetler yalnız bu uygulamanın
 * bildirimlerinden gelir.
 *
 * Eylem [ArkaKopru.eylemGonder] ile Rust'a gider (`src/eylem.rs`); süreçte
 * hangi çekirdek açıksa (pencereli ya da etkinliksiz) o uygular, uygulama
 * ekranı açılmaz. Çekirdek yoksa (süreç yeni açıldı ya da yoktu) eylem
 * şifreli kasaya yazılır ([Bekleyenler]), bildirim "Mailhere açılınca
 * uygulanacak" durumuna geçer ve arka plan hizmeti başlatılmaya çalışılır
 * (bildirim düğmesi arka plandan hizmet başlatmaya izin verir): hizmet
 * etkinliksiz çekirdeği açar, kuyruk boşalır ve eylem saniyeler içinde
 * uygulanır. Olmazsa uygulama açılınca uygulanır.
 *
 * "Kodu kopyala" kodu Rust'a sormaz: doğrulama kodu düğme niyetinin ekindedir
 * ve burada panoya yazılır; çekirdek kapalıyken de (yalnız bu alıcı ayaktayken)
 * çalışır. Kod günlüğe yazılmaz. Kopyalamadan sonra Rust'a yalnız posta
 * kimliğiyle `kodKopyalandi` eylemi gider: ayar açıksa ("kod kopyalanınca
 * okundu say") posta okundu sayılır; çekirdek yoksa öteki eylemler gibi
 * kasada bekler.
 */
class BildirimAlici : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            Bildirimler.EYLEM_ARKA_KAPAT -> {
                ArkaPlan.ayarla(ctx, false)
                return
            }
            Bildirimler.EYLEM_UYAN -> {
                ArkaPlan.uyan(ctx, goAsync())
                return
            }
        }
        val p = Bildirimler.postaOf(intent.extras) ?: return
        val olay = JSONObject().put("id", p.id).put("hesap", p.hesap)
        when (intent.action) {
            Bildirimler.EYLEM_SILINDI -> {
                Bildirimler.silindi(ctx, p)
                return
            }
            Bildirimler.EYLEM_KOD_KOPYALA -> {
                kodKopyala(ctx, p)
                return
            }
            Bildirimler.EYLEM_OKUNDU -> olay.put("tur", "okundu")
            Bildirimler.EYLEM_ARSIVLE -> olay.put("tur", "arsivle")
            Bildirimler.EYLEM_SIL -> olay.put("tur", "sil")
            Bildirimler.EYLEM_ISTENMEYEN -> olay.put("tur", "istenmeyen")
            Bildirimler.EYLEM_YANIT -> {
                val metin = RemoteInput.getResultsFromIntent(intent)
                    ?.getCharSequence(Bildirimler.ANAHTAR_YANIT)
                    ?.toString()
                    ?.trim()
                if (metin.isNullOrEmpty()) {
                    // Boş yanıt gönderilmez; bekleme göstergesi kalksın diye
                    // bildirim yerinde yenilenir.
                    Bildirimler.yenile(ctx, p)
                    return
                }
                olay.put("tur", "yanit").put("metin", metin)
            }
            Bildirimler.EYLEM_GERI_AL -> {
                val kuyruk = intent.getStringExtra(Bildirimler.E_KUYRUK) ?: return
                olay.put("tur", "geriAl").put("kuyruk", kuyruk)
            }
            else -> return
        }
        if (ArkaKopru.eylemGonder(olay)) return
        // Çekirdek yok: kasa işleri ana iş parçacığını tutmasın.
        val sonuc = goAsync()
        Thread {
            try {
                Bekleyenler.ekle(ctx, olay)
                Bildirimler.bekliyor(ctx, p)
                // Hizmet çekirdeği açar ve kuyruğu uygular (Android 12+ bildirim
                // düğmesinden hizmet başlatmaya izin verir).
                ArkaPlan.baslatGerekirse(ctx)
            } catch (e: Exception) {
                Log.w(TAG, "bildirim eylemi kaydedilemedi")
                Bildirimler.hata(ctx, p.id, ctx.getString(R.string.bildirim_kaydedilemedi))
            } finally {
                sonuc.finish()
            }
        }.start()
    }

    /**
     * Kodu panoya yazar ve kısa onay verir: bildirim "Kod kopyalandı"ya döner
     * ve kendiliğinden kalkar; süreli bildirimi olmayan eski Android'de
     * bildirim hemen kalkar, onay kısa iletiyle (Toast) verilir. Yazılamazsa
     * bildirim yerinde kalır, ne olduğunu ve ne yapılabileceğini söyler.
     * Ardından Rust'a `kodKopyalandi` eylemi gider (okundu sayma).
     */
    private fun kodKopyala(ctx: Context, p: Posta) {
        if (!Bildirimler.kodKopyala(ctx, p.kod)) {
            Bildirimler.hata(ctx, p.id, ctx.getString(R.string.bildirim_kod_kopyalanamadi))
            return
        }
        if (!Bildirimler.kodKopyalandi(ctx, p)) {
            Toast.makeText(ctx.applicationContext, R.string.bildirim_kod_kopyalandi, Toast.LENGTH_SHORT).show()
        }
        // Kod kendiliğinden kopyalanırken okundu sayılmış postada yapılacak iş yok.
        if (p.okundu) return
        // Ayar açıksa posta okundu sayılır (kararı Rust verir). "Kod kopyalandı"
        // onayına dokunulmaz; çekirdek yoksa eylem kasada bekler ve hizmet
        // başlatılmaya çalışılır, olmazsa uygulama açılınca uygulanır.
        val olay = JSONObject().put("tur", "kodKopyalandi").put("id", p.id)
        if (ArkaKopru.eylemGonder(olay)) return
        val sonuc = goAsync()
        Thread {
            try {
                Bekleyenler.ekle(ctx, olay)
                ArkaPlan.baslatGerekirse(ctx)
            } catch (e: Exception) {
                Log.w(TAG, "kod kopyalama eylemi kaydedilemedi")
            } finally {
                sonuc.finish()
            }
        }.start()
    }

    companion object {
        private const val TAG = "Mailhere"
    }
}

/**
 * Çekirdek yokken basılan bildirim eylemleri. Yanıt metni taşıyabildiği
 * için kasada (AndroidKeyStore, AES-GCM) tek kayıt olarak durur; çekirdek
 * açılınca `ArkaKopru.bekleyenler` ile boşaltılır. En çok [EN_COK] eylem.
 */
object Bekleyenler {
    private const val ANAHTAR = "bildirim-bekleyen"
    private const val EN_COK = 50
    private val kilit = Any()

    fun ekle(ctx: Context, olay: JSONObject) = synchronized(kilit) {
        val kasa = SecretVault(ctx)
        val liste = kasa.get(ANAHTAR)?.let { JSONArray(it) } ?: JSONArray()
        if (liste.length() < EN_COK) liste.put(olay)
        kasa.set(ANAHTAR, liste.toString())
    }

    fun bosalt(ctx: Context): JSONArray = synchronized(kilit) {
        val kasa = SecretVault(ctx)
        val liste = try {
            kasa.get(ANAHTAR)?.let { JSONArray(it) } ?: JSONArray()
        } catch (e: Exception) {
            // Çözülemeyen kayıt (anahtar yenilenmiş) atılır; eylemler kaybolur
            // ama bildirimler yerinde kalır, kullanıcı yeniden dener.
            JSONArray()
        }
        kasa.delete(ANAHTAR)
        liste
    }
}
