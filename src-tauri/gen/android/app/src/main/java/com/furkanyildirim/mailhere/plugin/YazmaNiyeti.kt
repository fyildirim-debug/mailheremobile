package com.furkanyildirim.mailhere.plugin

import android.content.Intent
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

/**
 * Etkinliğe gelen "yazma ekranını aç" niyetleri: başka uygulamadaki e-posta
 * bağlantısı (`ACTION_VIEW` / `ACTION_SENDTO`, `mailto:` şeması; manifestteki
 * niyet süzgeci). Niyet dış uygulamadan gelir, yani güvenilmez girdidir:
 * burada yalnız tanınır ve Rust'a aktarılacak olaya çevrilir; adres Rust'ta
 * ayrıştırılır ve doğrulanır (`src/mailto.rs`, birim testli). Hiçbir yol
 * istisna fırlatmaz; adres günlüğe yazılmaz.
 *
 * Olay `src/eylem.rs` `Eylem::Yaz` biçimindedir:
 * `{ tur: "yaz", adres?: "mailto:…", alicilar?: string[] }`.
 */
object YazmaNiyeti {
    private const val TAG = "Mailhere"
    private const val SEMA = "mailto:"

    /** `EXTRA_EMAIL` ekinden alınan en çok adres ve tek adresin en çok uzunluğu. */
    private const val EN_COK_EK = 20
    private const val EK_EN_UZUN = 512

    /**
     * Niyet eski: geri yüklenen etkinliğin niyeti
     * ([com.furkanyildirim.mailhere.MainActivity]) ya da süreç yeniden
     * başlatılırken ([com.furkanyildirim.mailhere.YenidenBaslatActivity])
     * özgün niyetin "son uygulamalardan açıldı" bilgisi (yeniden başlatma
     * niyetin bayraklarını korumaz). Eski niyet yazma ekranı açmaz.
     */
    const val E_GECMISTEN = "mh.gecmisten"

    /**
     * Niyet yazma ekranı istiyorsa Rust'a gidecek olay; değilse `null`.
     * Son uygulamalardan yeniden açılan ya da geri yüklenen görevde eski
     * niyet yok sayılır: aynı bağlantı ikinci kez yazma ekranı açmaz.
     */
    fun olay(intent: Intent?): JSONObject? {
        if (intent == null) return null
        return try {
            if (gecmisten(intent)) return null
            when (intent.action) {
                Intent.ACTION_VIEW, Intent.ACTION_SENDTO -> postaBaglantisi(intent)
                else -> null
            }
        } catch (e: Exception) {
            // Bozuk ek (çözülemeyen Parcelable vb.): niyet yok sayılır.
            Log.w(TAG, "yazma niyeti okunamadı: ${e.javaClass.simpleName}")
            null
        }
    }

    private fun gecmisten(intent: Intent): Boolean =
        intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0 ||
            intent.getBooleanExtra(E_GECMISTEN, false)

    /** `mailto:` adresi ve (varsa) `EXTRA_EMAIL` eki; başka şema `null`. */
    private fun postaBaglantisi(intent: Intent): JSONObject? {
        val adres = intent.dataString ?: return null
        if (!adres.startsWith(SEMA, ignoreCase = true)) return null
        val olay = JSONObject().put("tur", "yaz").put("adres", adres)
        val ek = intent.getStringArrayExtra(Intent.EXTRA_EMAIL)
        if (!ek.isNullOrEmpty()) {
            val alicilar = JSONArray()
            for (a in ek.filterNotNull().take(EN_COK_EK)) {
                if (a.length <= EK_EN_UZUN) alicilar.put(a)
            }
            olay.put("alicilar", alicilar)
        }
        return olay
    }
}
