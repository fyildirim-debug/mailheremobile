package com.furkanyildirim.mailhere.plugin

import org.json.JSONObject

/**
 * Sürecin yaşam durumu (süreç başına; `MainActivity`, hizmet ve alıcı
 * paylaşır):
 *
 * - [etkinlikVar]: `MainActivity` yaşıyor (yok edilmemiş). Hizmet kapanırken
 *   süreci sonlandırma kararı buna bakar.
 * - [yokEdildi]: bu süreçte ilk etkinlik yok edildi. Tauri aynı süreçte yeni
 *   etkinliğe webview kuramaz (bkz. `docs/ANDROID.md` §3 "Arka plan"); bu
 *   bayrak doluyken açılan `MainActivity` süreci [YenidenBaslatActivity] ile
 *   temiz başlatır.
 * - [acilacak]: soğuk açılışta dokunulan bildirimin postası; Rust çekirdeği
 *   açılınca `ArkaKopru.bekleyenler` ile alınır.
 * - [yazilacak]: çekirdek dinlemezken (soğuk açılış, parola kilidi) gelen
 *   yazma isteği (`mailto:` bağlantısı; [YazmaNiyeti] olayı); aynı yoldan
 *   alınır. Yalnız bellekte durur, en son istek geçerlidir.
 */
object Yasam {
    @Volatile
    var etkinlikVar = false

    @Volatile
    var yokEdildi = false

    private var acilacak: Long? = null

    fun acilacakYaz(id: Long) = synchronized(this) { acilacak = id }

    fun acilacakAl(): Long? = synchronized(this) {
        val v = acilacak
        acilacak = null
        v
    }

    private var yazilacak: JSONObject? = null

    fun yazilacakYaz(olay: JSONObject) = synchronized(this) { yazilacak = olay }

    fun yazilacakAl(): JSONObject? = synchronized(this) {
        val v = yazilacak
        yazilacak = null
        v
    }
}
