package com.furkanyildirim.mailhere.plugin

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.app.RemoteInput
import app.tauri.plugin.JSObject
import com.furkanyildirim.mailhere.R
import org.json.JSONArray
import org.json.JSONObject

/**
 * Bildirim düğmeleri (Okundu, Arşivle, Yanıtla, Geri al), kaydırıp kapatma,
 * anlık bildirimin "Kapat" düğmesi ve uyanma alarmı. Dışa kapalıdır
 * (`exported=false`); niyetler yalnız bu uygulamanın bildirimlerinden gelir.
 *
 * Süreç ve Rust çekirdeği çalışıyorsa eylem [MhPlugin.ilet] ile çekirdeğe
 * gider (`src/eylem.rs`); uygulama ekranı açılmaz. Süreç ölmüşse çekirdek
 * buradan başlatılamaz (Rust çalışma ortamı yalnız etkinlikle açılır): eylem
 * şifreli kasaya yazılır ([Bekleyenler]), bildirim "Mailhere açılınca
 * uygulanacak" durumuna geçer; uygulama açılınca kuyruk boşalır.
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
        val olay = JSObject().put("id", p.id).put("hesap", p.hesap)
        when (intent.action) {
            Bildirimler.EYLEM_SILINDI -> {
                Bildirimler.silindi(ctx, p)
                return
            }
            Bildirimler.EYLEM_OKUNDU -> olay.put("tur", "okundu")
            Bildirimler.EYLEM_ARSIVLE -> olay.put("tur", "arsivle")
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
        if (MhPlugin.ilet(olay)) return
        // Süreç kapalıydı: kasa işleri ana iş parçacığını tutmasın.
        val sonuc = goAsync()
        Thread {
            try {
                Bekleyenler.ekle(ctx, olay)
                Bildirimler.bekliyor(ctx, p)
            } catch (e: Exception) {
                Log.w(TAG, "bildirim eylemi kaydedilemedi")
                Bildirimler.hata(ctx, p.id, ctx.getString(R.string.bildirim_kaydedilemedi))
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
 * Süreç kapalıyken basılan bildirim eylemleri. Yanıt metni taşıyabildiği
 * için kasada (AndroidKeyStore, AES-GCM) tek kayıt olarak durur; uygulama
 * açılınca `bekleyenEylemler` ile boşaltılır. En çok [EN_COK] eylem.
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
