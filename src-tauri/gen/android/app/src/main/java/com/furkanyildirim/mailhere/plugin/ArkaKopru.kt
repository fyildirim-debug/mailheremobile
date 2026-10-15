package com.furkanyildirim.mailhere.plugin

import android.content.Context
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import com.furkanyildirim.mailhere.R
import org.json.JSONArray
import org.json.JSONObject

/**
 * Etkinliksiz JNI köprüsü (Rust `src/kopru.rs`). Tauri'nin eklenti köprüsü
 * etkinlik ister ve etkinlik yok edilmişken panikler; çekirdeğin kendi
 * başlattığı işler — kasa, yeni posta bildirimi, bildirimleri uzlaştırma,
 * bildirim eylemlerinin sonucu — bu yüzden etkinlik olsa da olmasa da
 * buradan geçer. Rust tarafı sınıfı `kur` sırasında aldığı `GlobalRef` ile,
 * uygulama `Context`iyle çağırır; hangi iş parçacığında olduğu önemsizdir.
 *
 * - `external` yöntemler Rust'ın girişleri (`Java_…_ArkaKopru_<ad>`); Rust'taki
 *   `kopru.rs` birim testi adları bu dosyayla eşler, `scripts/apk-denetle.sh`
 *   sembolleri `.so` içinde arar.
 * - `@JvmStatic fun` yöntemler Rust'tan çağrılır; imzalar `kopru.rs`
 *   `KOTLIN_YONTEMLER` ile aynıdır. Fırlattıkları istisna Rust'ta hataya
 *   döner (metinler gizli değer taşımaz).
 * - Kotlin'in Rust'a giden çağrıları [guvenli] ile sarılır: kitaplık
 *   yüklenemezse (`UnsatisfiedLinkError`) ya da beklenmeyen bir `Throwable`
 *   gelirse uygulama çökmez, varsayılan döner.
 */
object ArkaKopru {
    private const val TAG = "Mailhere"
    private const val KITAPLIK = "mailhere_lib"

    /** `cekirdekBaslat` / `tekSefer` sonuç kodları (Rust `cekirdek::Baslatma`). */
    const val KOD_BASLADI = 0
    const val KOD_ZATEN = 1
    const val KOD_KOPRU_YOK = 2
    const val KOD_ANAHTAR_YOK = 3
    const val KOD_HESAP_YOK = 4
    const val KOD_HATA = 5
    const val KOD_VAZGECILDI = 6

    /** `cekirdekDurumu` (Rust `cekirdek::Durum`). */
    const val DURUM_YOK = 0
    const val DURUM_BASLIKSIZ = 1
    const val DURUM_TAURI = 2

    @Volatile
    private var yuklu = false

    /** Kasa işlemleri sırayla: Keystore çağrıları eşzamanlı yığılmasın. */
    private val kasaKilidi = Any()

    /**
     * Yerel kitaplığı yükler (Tauri'nin `Rust` sınıfı da aynı kitaplığı yükler;
     * ikinci yükleme etkisizdir). Başarısızsa `false`; çökme yok.
     */
    @JvmStatic
    fun yukle(): Boolean {
        if (yuklu) return true
        return try {
            System.loadLibrary(KITAPLIK)
            yuklu = true
            true
        } catch (e: Throwable) {
            Log.e(TAG, "yerel kitaplık yüklenemedi: ${e.javaClass.simpleName}")
            false
        }
    }

    // ------------------------------------------------------- Rust girişleri

    /** `Application.onCreate`: Context, veri ve önbellek klasörleri; günlük ve çalışma ortamı. */
    @JvmStatic external fun kur(ctx: Context, veri: String, onbellek: String)

    /** Etkinliksiz çekirdeği açar ve başlatır; sonuç `KOD_*`. */
    @JvmStatic external fun cekirdekBaslat(): Int

    /** Etkinliksiz çekirdeği durdurur (varsa). */
    @JvmStatic external fun cekirdekDurdur()

    /** `DURUM_*`. */
    @JvmStatic external fun cekirdekDurumu(): Int

    /** Çalışan çekirdeğe "şimdi eşitle"; çekirdek yoksa `false`. */
    @JvmStatic external fun esitle(): Boolean

    /** Bildirim eylemi (JSON, `src/eylem.rs`); çekirdek yoksa `false`. */
    @JvmStatic external fun eylem(json: String): Boolean

    /** Tek seferlik eşitle-bildir (en çok `sureMs`); sonuç `KOD_*`. */
    @JvmStatic external fun tekSefer(sureMs: Long): Int

    private inline fun <T> guvenli(varsayilan: T, f: () -> T): T {
        if (!yukle()) return varsayilan
        return try {
            f()
        } catch (e: Throwable) {
            Log.w(TAG, "köprü çağrısı başarısız: ${e.javaClass.simpleName}")
            varsayilan
        }
    }

    fun durum(): Int = guvenli(DURUM_YOK) { cekirdekDurumu() }

    fun cekirdekVar(): Boolean = durum() != DURUM_YOK

    fun baslat(): Int = guvenli(KOD_HATA) { cekirdekBaslat() }

    fun durdur() = guvenli(Unit) { cekirdekDurdur() }

    fun esitleGuvenli(): Boolean = guvenli(false) { esitle() }

    fun eylemGonder(olay: JSONObject): Boolean = guvenli(false) { eylem(olay.toString()) }

    fun tekSeferGuvenli(sureMs: Long): Int = guvenli(KOD_HATA) { tekSefer(sureMs) }

    // ------------------------------------------------ Rust'tan çağrılanlar

    /** `SecretVault.get`: yoksa `null`; çözülemiyorsa [VaultException]. */
    @JvmStatic fun kasaOku(ctx: Context, ad: String): String? =
        synchronized(kasaKilidi) { SecretVault(ctx).get(ad) }

    @JvmStatic fun kasaYaz(ctx: Context, ad: String, deger: String) =
        synchronized(kasaKilidi) { SecretVault(ctx).set(ad, deger) }

    @JvmStatic fun kasaSil(ctx: Context, ad: String) =
        synchronized(kasaKilidi) { SecretVault(ctx).delete(ad) }

    /**
     * Yeni posta bildirimleri: `{ gruplar: [{ hesap, hesapAdi, eylemler: string[],
     * postalar: [{ id, gonderen, konu, onizleme, zaman }] }] }` (`src/notify.rs`).
     */
    @JvmStatic fun bildir(ctx: Context, json: String) {
        val gruplar = JSONObject(json).optJSONArray("gruplar") ?: return
        for (i in 0 until gruplar.length()) {
            val g = gruplar.optJSONObject(i) ?: continue
            val hesap = g.optString("hesap")
            val hesapAdi = g.optString("hesapAdi")
            val eylemler = metinler(g.optJSONArray("eylemler"))
            val liste = g.optJSONArray("postalar") ?: continue
            val postalar = ArrayList<Posta>(liste.length())
            for (j in 0 until liste.length()) {
                val p = liste.optJSONObject(j) ?: continue
                postalar += Posta(
                    id = p.optLong("id", -1),
                    hesap = hesap,
                    hesapAdi = hesapAdi,
                    gonderen = p.optString("gonderen"),
                    konu = p.optString("konu"),
                    onizleme = p.optString("onizleme"),
                    zaman = p.optLong("zaman", 0),
                    eylemler = eylemler,
                )
            }
            Bildirimler.postaGoster(ctx, hesap, hesapAdi, postalar.filter { it.id >= 0 })
        }
    }

    /** Postaların bildirimlerini kaldırır: `[id, …]`. */
    @JvmStatic fun kaldir(ctx: Context, json: String) {
        val ids = JSONArray(json)
        Bildirimler.kaldir(ctx, (0 until ids.length()).map { ids.getLong(it) })
    }

    /** Eylem uygulanamadı: bildirimde neden ve çözüm. */
    @JvmStatic fun hata(ctx: Context, id: Long, metin: String) {
        Bildirimler.hata(ctx, id, metin.ifBlank { ctx.getString(R.string.bildirim_hata) })
    }

    @JvmStatic fun yanitKuyrukta(ctx: Context, id: Long, kuyruk: String, gonderim: Long, metin: String) {
        Bildirimler.yanitKuyrukta(ctx, id, kuyruk, gonderim, metin)
    }

    @JvmStatic fun yanitGeriAlindi(ctx: Context, id: Long, iptal: Boolean) {
        Bildirimler.yanitGeriAlindi(ctx, id, iptal)
    }

    /** Hâlâ "yeni posta" durumundaki bildirimler: `[id, …]`. */
    @JvmStatic fun etkin(ctx: Context): String = JSONArray(Bildirimler.etkinPostalar(ctx)).toString()

    /** Uygulamanın ve kanalın bildirimleri açık mı (kullanıcı kapatmış olabilir). */
    @JvmStatic fun bildirimlerAcik(ctx: Context, kanal: String): Boolean {
        val manager = NotificationManagerCompat.from(ctx)
        // Kanal henüz yoksa (ilk açılış) uygulama düzeyindeki durum geçerlidir.
        val kanalKapali = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            manager.getNotificationChannel(kanal)?.importance == NotificationManagerCompat.IMPORTANCE_NONE
        return manager.areNotificationsEnabled() && !kanalKapali
    }

    /**
     * Çekirdek yokken alınan eylemler (kasadan; alınınca silinir) ve soğuk
     * açılışta dokunulan bildirim: `[{ tur, id, … }, …]`.
     */
    @JvmStatic fun bekleyenler(ctx: Context): String {
        val eylemler = try {
            Bekleyenler.bosalt(ctx)
        } catch (e: Exception) {
            JSONArray()
        }
        Yasam.acilacakAl()?.let { eylemler.put(JSONObject().put("tur", "ac").put("id", it)) }
        return eylemler.toString()
    }

    /** "Anlık bildirim" açık mı (seçilmemişse açık). */
    @JvmStatic fun anlikAcik(ctx: Context): Boolean = ArkaPlan.acik(ctx)

    private fun metinler(a: JSONArray?): List<String> {
        if (a == null) return emptyList()
        return (0 until a.length()).mapNotNull { a.optString(it).ifBlank { null } }
    }
}
