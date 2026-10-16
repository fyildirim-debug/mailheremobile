package com.furkanyildirim.mailhere.plugin

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.StyleSpan
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.RemoteInput
import androidx.core.content.ContextCompat
import com.furkanyildirim.mailhere.MainActivity
import com.furkanyildirim.mailhere.R

/**
 * Bildirimi gösterilen posta (`src/notify.rs` → `Kart`, hesap bilgisi ve
 * kullanıcının seçtiği düğmelerle). [eylemler] Kotlin düğme adlarıdır
 * (`yanit`, `okundu`, `arsivle`, `sil`, `istenmeyen`), kullanıcının sırasıyla.
 */
data class Posta(
    val id: Long,
    val hesap: String,
    val hesapAdi: String,
    val gonderen: String,
    val konu: String,
    val onizleme: String,
    val zaman: Long,
    val eylemler: List<String> = emptyList(),
)

/**
 * Yeni posta bildirimleri (`NotificationCompat`). Rust yalnız veri gönderir
 * (`src/notify.rs`, `src/eylem.rs`; etkinliksiz köprü [ArkaKopru]); metinler
 * burada, `strings.xml`dedir.
 *
 * - Posta başına bir bildirim (etiket `posta:<id>`), hesap başına grup
 *   (`mailhere:<hesap>`) ve sabit etiketli özet (`ozet:<hesap>`, InboxStyle:
 *   "N yeni posta", satır satır gönderen ve konu).
 * - Bir yığında yalnız en yeni posta ses çıkarır ve üstten görünür; öteki
 *   postalar ve özet sessizdir (`GROUP_ALERT_CHILDREN` + `setSilent`).
 * - Kilit ekranı: `VISIBILITY_PRIVATE` + açık sürüm (yalnız hesap adı ve
 *   "Yeni posta"); kullanıcı "hassas içeriği gizle" seçtiyse o görünür.
 * - Düğmeler kullanıcının ayarından (Ayarlar › Bildirimler; varsayılan
 *   Yanıtla, Sil, İstenmeyen; en çok üç): Yanıtla satır içi (RemoteInput),
 *   Okundu, Arşivle, Sil, İstenmeyen ([BildirimAlici]). Düğme listesi
 *   bildirimin eklerinde saklanır; bildirim yeniden çizilince (hata, boş
 *   yanıt) aynı düğmeler kurulur. Dokununca posta açılır ([MainActivity] →
 *   [MhPlugin]).
 *
 * Özet, etkin bildirimlerden yeniden kurulur; `notify`/`cancel` eşzamansız
 * olduğundan eklenen ve çıkan etiketler ayrıca verilir.
 */
object Bildirimler {
    const val KANAL_POSTA = "yeni-posta"
    /**
     * Ön plan hizmetinin kanalı: en düşük önem (durum çubuğunda simge yok,
     * bildirim panelinin en altında katlı). Kanal önemi oluşturulduktan sonra
     * değiştirilemediğinden yeni kimlik; eski (düşük önemli) kanal silinir.
     */
    const val KANAL_ARKA = "arka-plan-sessiz"
    private const val KANAL_ARKA_ESKI = "arka-plan"

    /** Ön plan hizmetinin (anlık bildirim) kalıcı bildirimi; etiketsiz. */
    const val ID_ARKA = 7

    /** Arka plandan hizmet başlatılamadı: yedek (yoklama) açıklaması; etiketsiz. */
    const val ID_YEDEK = 8

    /**
     * Parola kilidi: telefon yeniden başladı ama anahtar parolayla sarılı,
     * çekirdek açılamıyor; "Mailhere kilitli — postaları almak için bir kez
     * aç". Kendi sessiz kanalı (varsayılan önem, ses yok); dokununca uygulama
     * (kilit ekranı). Kilit açılınca kalkar.
     */
    const val KANAL_KILIT = "kilit"
    const val ID_KILIT = 9

    private const val ETIKET_POSTA = "posta:"
    private const val ETIKET_OZET = "ozet:"
    private const val GRUP = "mailhere:"
    private const val TAG = "Mailhere"

    /** Özette gösterilen en çok satır. */
    private const val SATIR = 6

    /** Android bildirimde en çok üç düğme gösterir (`mh_core::NOTIFY_ACTIONS_MAX`). */
    private const val EN_COK_DUGME = 3

    // Bildirim ve niyet ekleri.
    const val E_ID = "mh.id"
    const val E_HESAP = "mh.hesap"
    const val E_HESAP_ADI = "mh.hesapAdi"
    const val E_GONDEREN = "mh.gonderen"
    const val E_KONU = "mh.konu"
    const val E_ONIZLEME = "mh.onizleme"
    const val E_ZAMAN = "mh.zaman"
    const val E_EYLEMLER = "mh.eylemler"
    const val E_KUYRUK = "mh.kuyruk"

    /**
     * Bildirimin durumu: yoksa yeni posta (özette sayılır, eylemleri var);
     * `yanit` yanıt giden kutusunda, `bilgi` kısa bilgi, `bekliyor` çekirdek
     * yokken alınan eylem çekirdek açılınca uygulanacak.
     */
    private const val E_DURUM = "mh.durum"
    private const val YANIT = "yanit"
    private const val BILGI = "bilgi"
    private const val BEKLIYOR = "bekliyor"

    /** RemoteInput sonucu. */
    const val ANAHTAR_YANIT = "mh.yanit"

    // Düğme adları (Rust `notify::eylem_adlari` ile aynı).
    const val DUGME_YANIT = "yanit"
    const val DUGME_OKUNDU = "okundu"
    const val DUGME_ARSIVLE = "arsivle"
    const val DUGME_SIL = "sil"
    const val DUGME_ISTENMEYEN = "istenmeyen"

    // Niyet eylemleri (uygulamaya özel; alıcı dışa kapalı).
    const val EYLEM_AC = "com.furkanyildirim.mailhere.POSTA_AC"
    const val EYLEM_UYGULAMA = "com.furkanyildirim.mailhere.UYGULAMA"
    const val EYLEM_OKUNDU = "com.furkanyildirim.mailhere.OKUNDU"
    const val EYLEM_ARSIVLE = "com.furkanyildirim.mailhere.ARSIVLE"
    const val EYLEM_SIL = "com.furkanyildirim.mailhere.SIL"
    const val EYLEM_ISTENMEYEN = "com.furkanyildirim.mailhere.ISTENMEYEN"
    const val EYLEM_YANIT = "com.furkanyildirim.mailhere.YANIT"
    const val EYLEM_GERI_AL = "com.furkanyildirim.mailhere.GERI_AL"
    const val EYLEM_SILINDI = "com.furkanyildirim.mailhere.SILINDI"
    const val EYLEM_ARKA_KAPAT = "com.furkanyildirim.mailhere.ARKA_KAPAT"
    const val EYLEM_UYAN = "com.furkanyildirim.mailhere.UYAN"

    /** Kanallar: yeni posta (yüksek) ve arka plan bağlantısı (en düşük, sessiz, simgesiz). */
    fun kanallar(ctx: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        val posta = NotificationChannel(
            KANAL_POSTA,
            ctx.getString(R.string.kanal_posta),
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = ctx.getString(R.string.kanal_posta_aciklama)
        }
        val arka = NotificationChannel(
            KANAL_ARKA,
            ctx.getString(R.string.kanal_arka),
            NotificationManager.IMPORTANCE_MIN,
        ).apply {
            description = ctx.getString(R.string.kanal_arka_aciklama)
            setShowBadge(false)
            setSound(null, null)
            enableVibration(false)
        }
        // Var olan kanalın yalnız adı ve açıklaması güncellenir; kullanıcının
        // seçtiği önem ve ses korunur.
        nm.createNotificationChannels(listOf(posta, arka))
        // Eski kanalda ön plan hizmeti bildirimi varken Android silmeye izin
        // vermez (yalnız önceki sürümün sürecinde olur); sonraki açılışta silinir.
        try {
            nm.deleteNotificationChannel(KANAL_ARKA_ESKI)
        } catch (e: SecurityException) {
            Log.w(TAG, "eski arka plan kanalı silinemedi")
        }
    }

    /** Bir hesabın yeni postaları (eskiden yeniye) ve özeti. */
    fun postaGoster(ctx: Context, hesap: String, hesapAdi: String, postalar: List<Posta>) {
        if (postalar.isEmpty()) return
        val enYeni = postalar.maxByOrNull { it.zaman }
        for (p in postalar) {
            gonder(ctx, ETIKET_POSTA + p.id, yeniPosta(ctx, p, sessiz = p !== enYeni))
        }
        ozetiYenile(ctx, hesap, hesapAdi, postalar, emptySet(), olustur = true)
    }

    /** Postanın bildirimini sessizce yeniden çizer (ör. boş yanıttan sonra). */
    fun yenile(ctx: Context, p: Posta) {
        gonder(ctx, ETIKET_POSTA + p.id, yeniPosta(ctx, p, sessiz = true))
    }

    /** Postaların bildirimlerini kaldırır, özetleri günceller. */
    fun kaldir(ctx: Context, ids: List<Long>) {
        val nm = NotificationManagerCompat.from(ctx)
        val etkin = etkinler(ctx)
        val cikan = mutableSetOf<String>()
        val hesaplar = mutableSetOf<String>()
        for (id in ids) {
            val etiket = ETIKET_POSTA + id
            etkin.firstOrNull { it.tag == etiket }?.notification?.extras?.getString(E_HESAP)?.let { hesaplar += it }
            cikan += etiket
            nm.cancel(etiket, 0)
        }
        for (h in hesaplar) ozetiYenile(ctx, h, null, emptyList(), cikan, olustur = false)
    }

    /** Kullanıcı bildirimi kaydırıp kapattı: özet güncellenir (yeniden kurulmaz). */
    fun silindi(ctx: Context, p: Posta) {
        ozetiYenile(ctx, p.hesap, p.hesapAdi, emptyList(), setOf(ETIKET_POSTA + p.id), olustur = false)
    }

    /** Bildirime dokunuldu: posta açılır, bildirim kalkar. */
    fun dokunuldu(ctx: Context, id: Long, hesap: String?) {
        val etiket = ETIKET_POSTA + id
        NotificationManagerCompat.from(ctx).cancel(etiket, 0)
        if (hesap != null) ozetiYenile(ctx, hesap, null, emptyList(), setOf(etiket), olustur = false)
    }

    /** Eylem uygulanamadı: bildirim yerinde kalır, nedeni ve çözümü yazar. */
    fun hata(ctx: Context, id: Long, metin: String) {
        val p = bul(ctx, id) ?: return
        val b = postaKurucu(ctx, p)
            .setContentText(metin)
            .setStyle(NotificationCompat.BigTextStyle().bigText(metin))
            .setContentIntent(acmaNiyeti(ctx, p))
            .setDeleteIntent(alici(ctx, EYLEM_SILINDI, p))
            .setAutoCancel(true)
            .setSilent(true)
        eylemler(ctx, b, p)
        gonder(ctx, ETIKET_POSTA + p.id, b.build())
    }

    /**
     * Yanıt giden kutusunda: geri sayım (`gonderim` anına) ve "Geri al".
     * Süre bitince bildirim kendiliğinden kalkar (API 26+), yanıt gider.
     */
    fun yanitKuyrukta(ctx: Context, id: Long, kuyruk: String, gonderim: Long, metin: String) {
        val p = bul(ctx, id) ?: return
        val kalan = gonderim - System.currentTimeMillis()
        val b = postaKurucu(ctx, p, YANIT)
            .setContentIntent(acmaNiyeti(ctx, p))
            .setRemoteInputHistory(arrayOf<CharSequence>(metin))
            .setAutoCancel(true)
            .setSilent(true)
        if (kalan > 1_000) {
            b.setContentText(ctx.getString(R.string.bildirim_yanit_kuyrukta))
                .setWhen(gonderim)
                .setUsesChronometer(true)
                .setChronometerCountDown(true)
                .setTimeoutAfter(kalan + 1_500)
                .addAction(
                    NotificationCompat.Action.Builder(
                        R.drawable.ic_geri_al,
                        ctx.getString(R.string.bildirim_geri_al),
                        alici(ctx, EYLEM_GERI_AL, p) { it.putExtra(E_KUYRUK, kuyruk) },
                    ).setShowsUserInterface(false).build(),
                )
        } else {
            b.setContentText(ctx.getString(R.string.bildirim_yanit_gidiyor)).setTimeoutAfter(4_000)
        }
        gonder(ctx, ETIKET_POSTA + p.id, b.build())
        ozetiYenile(ctx, p.hesap, p.hesapAdi, emptyList(), setOf(ETIKET_POSTA + p.id), olustur = false)
    }

    /** Geri alma sonucu kısa süre görünür. */
    fun yanitGeriAlindi(ctx: Context, id: Long, iptal: Boolean) {
        val p = bul(ctx, id) ?: return
        val metin = ctx.getString(if (iptal) R.string.bildirim_yanit_geri_alindi else R.string.bildirim_yanit_gec)
        val b = postaKurucu(ctx, p, BILGI)
            .setContentText(metin)
            .setStyle(NotificationCompat.BigTextStyle().bigText(metin))
            .setContentIntent(acmaNiyeti(ctx, p))
            .setAutoCancel(true)
            .setSilent(true)
            .setTimeoutAfter(6_000)
        gonder(ctx, ETIKET_POSTA + p.id, b.build())
    }

    /**
     * Çekirdek yokken basılan eylem kasaya yazıldı: Mailhere açılınca (ya da
     * arka plan hizmeti çekirdeği açınca) uygulanacağı yazar; dokununca
     * uygulama açılır ve kuyruk boşalır.
     */
    fun bekliyor(ctx: Context, p: Posta) {
        val metin = ctx.getString(R.string.bildirim_bekliyor)
        val b = postaKurucu(ctx, p, BEKLIYOR)
            .setContentText(metin)
            .setStyle(NotificationCompat.BigTextStyle().bigText(metin))
            .setContentIntent(acmaNiyeti(ctx, p))
            .setAutoCancel(true)
            .setSilent(true)
        gonder(ctx, ETIKET_POSTA + p.id, b.build())
        ozetiYenile(ctx, p.hesap, p.hesapAdi, emptyList(), setOf(ETIKET_POSTA + p.id), olustur = false)
    }

    /** Hâlâ "yeni posta" durumundaki bildirimlerin posta kimlikleri. */
    fun etkinPostalar(ctx: Context): List<Long> =
        etkinler(ctx)
            .filter { it.tag?.startsWith(ETIKET_POSTA) == true && it.notification.extras.getString(E_DURUM) == null }
            .map { it.notification.extras.getLong(E_ID, -1) }
            .filter { it >= 0 }

    /** Niyet ya da bildirim eklerinden posta; eksikse `null`. */
    fun postaOf(x: Bundle?): Posta? {
        if (x == null) return null
        val id = x.getLong(E_ID, -1)
        val hesap = x.getString(E_HESAP) ?: return null
        if (id < 0) return null
        return Posta(
            id = id,
            hesap = hesap,
            hesapAdi = x.getString(E_HESAP_ADI) ?: "",
            gonderen = x.getString(E_GONDEREN) ?: "",
            konu = x.getString(E_KONU) ?: "",
            onizleme = x.getString(E_ONIZLEME) ?: "",
            zaman = x.getLong(E_ZAMAN, 0),
            eylemler = x.getStringArray(E_EYLEMLER)?.toList() ?: emptyList(),
        )
    }

    private fun ekler(p: Posta): Bundle = Bundle().apply {
        putLong(E_ID, p.id)
        putString(E_HESAP, p.hesap)
        putString(E_HESAP_ADI, p.hesapAdi)
        putString(E_GONDEREN, p.gonderen)
        putString(E_KONU, p.konu)
        putString(E_ONIZLEME, p.onizleme)
        putLong(E_ZAMAN, p.zaman)
        putStringArray(E_EYLEMLER, p.eylemler.toTypedArray())
    }

    private fun vurgu(ctx: Context) = ContextCompat.getColor(ctx, R.color.vurgu)

    private fun konuOf(ctx: Context, p: Posta) = p.konu.ifBlank { ctx.getString(R.string.bildirim_konu_yok) }

    private fun baslikOf(ctx: Context, p: Posta) = p.gonderen.ifBlank { ctx.getString(R.string.app_name) }

    /** Kilit ekranındaki açık sürüm: hesap adı ve kısa metin, içerik yok. */
    private fun acikSurum(ctx: Context, hesapAdi: String, metin: String): Notification =
        NotificationCompat.Builder(ctx, KANAL_POSTA)
            .setSmallIcon(R.drawable.ic_bildirim)
            .setColor(vurgu(ctx))
            .setContentTitle(hesapAdi.ifBlank { ctx.getString(R.string.app_name) })
            .setContentText(metin)
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .build()

    /** Posta bildirimlerinin ortak kurucusu. */
    private fun postaKurucu(ctx: Context, p: Posta, durum: String? = null): NotificationCompat.Builder {
        val x = ekler(p)
        if (durum != null) x.putString(E_DURUM, durum)
        return NotificationCompat.Builder(ctx, KANAL_POSTA)
            .setSmallIcon(R.drawable.ic_bildirim)
            .setColor(vurgu(ctx))
            .setContentTitle(baslikOf(ctx, p))
            .setSubText(p.hesapAdi.ifBlank { null })
            .setWhen(p.zaman)
            .setShowWhen(true)
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setGroup(GRUP + p.hesap)
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            .setOnlyAlertOnce(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(acikSurum(ctx, p.hesapAdi, ctx.getString(R.string.bildirim_yeni_posta)))
            .addExtras(x)
    }

    /** Konu kalın, altında önizlemenin ilk satırı. */
    private fun govde(konu: String, onizleme: String): CharSequence {
        val s = SpannableStringBuilder(konu)
        s.setSpan(StyleSpan(Typeface.BOLD), 0, konu.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        if (onizleme.isNotBlank()) s.append('\n').append(onizleme)
        return s
    }

    private fun yeniPosta(ctx: Context, p: Posta, sessiz: Boolean): Notification {
        val konu = konuOf(ctx, p)
        val b = postaKurucu(ctx, p)
            .setContentText(konu)
            .setStyle(NotificationCompat.BigTextStyle().bigText(govde(konu, p.onizleme)))
            .setContentIntent(acmaNiyeti(ctx, p))
            .setDeleteIntent(alici(ctx, EYLEM_SILINDI, p))
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
        eylemler(ctx, b, p)
        // Yığının yalnız en yenisi ses çıkarır (Android 7 için de: kanal yok).
        if (sessiz) b.setSilent(true) else b.setDefaults(NotificationCompat.DEFAULT_ALL)
        return b.build()
    }

    /** Kullanıcının seçtiği düğmeler, kendi sırasıyla; en çok üç. */
    private fun eylemler(ctx: Context, b: NotificationCompat.Builder, p: Posta) {
        for (ad in p.eylemler.distinct().take(EN_COK_DUGME)) {
            val eylem = when (ad) {
                DUGME_YANIT -> yanitEylemi(ctx, p)
                DUGME_OKUNDU -> dugme(ctx, p, R.drawable.ic_okundu, R.string.bildirim_okundu, EYLEM_OKUNDU, NotificationCompat.Action.SEMANTIC_ACTION_MARK_AS_READ)
                DUGME_ARSIVLE -> dugme(ctx, p, R.drawable.ic_arsiv, R.string.bildirim_arsivle, EYLEM_ARSIVLE, NotificationCompat.Action.SEMANTIC_ACTION_ARCHIVE)
                DUGME_SIL -> dugme(ctx, p, R.drawable.ic_sil, R.string.bildirim_sil, EYLEM_SIL, NotificationCompat.Action.SEMANTIC_ACTION_DELETE)
                DUGME_ISTENMEYEN -> dugme(ctx, p, R.drawable.ic_istenmeyen, R.string.bildirim_istenmeyen, EYLEM_ISTENMEYEN, NotificationCompat.Action.SEMANTIC_ACTION_NONE)
                else -> continue
            }
            b.addAction(eylem)
        }
    }

    private fun dugme(
        ctx: Context,
        p: Posta,
        ikon: Int,
        metin: Int,
        eylem: String,
        anlam: Int,
    ): NotificationCompat.Action =
        NotificationCompat.Action.Builder(ikon, ctx.getString(metin), alici(ctx, eylem, p))
            .setSemanticAction(anlam)
            .setShowsUserInterface(false)
            .build()

    private fun yanitEylemi(ctx: Context, p: Posta): NotificationCompat.Action {
        val giris = RemoteInput.Builder(ANAHTAR_YANIT)
            .setLabel(ctx.getString(R.string.bildirim_yanit_ipucu))
            .build()
        return NotificationCompat.Action.Builder(R.drawable.ic_yanit, ctx.getString(R.string.bildirim_yanitla), alici(ctx, EYLEM_YANIT, p, degisken = true))
            .addRemoteInput(giris)
            .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY)
            // Sistem önerili yanıt üretmez: ne yazılacağına kullanıcı karar verir.
            .setAllowGeneratedReplies(false)
            .setShowsUserInterface(false)
            .build()
    }

    /** Özet: etkin yeni posta bildirimleri (− çıkan, + eklenen) üzerinden. */
    @SuppressLint("MissingPermission")
    private fun ozetiYenile(
        ctx: Context,
        hesap: String,
        hesapAdi: String?,
        eklenen: List<Posta>,
        cikan: Set<String>,
        olustur: Boolean,
    ) {
        val etiket = ETIKET_OZET + hesap
        val etkin = etkinler(ctx)
        val satirlar = LinkedHashMap<String, Posta>()
        for (sbn in etkin) {
            val t = sbn.tag ?: continue
            if (!t.startsWith(ETIKET_POSTA) || t in cikan) continue
            val x = sbn.notification.extras
            if (x.getString(E_HESAP) != hesap || x.getString(E_DURUM) != null) continue
            postaOf(x)?.let { satirlar[t] = it }
        }
        for (p in eklenen) satirlar[ETIKET_POSTA + p.id] = p
        val nm = NotificationManagerCompat.from(ctx)
        if (satirlar.isEmpty()) {
            nm.cancel(etiket, 0)
            return
        }
        // Kaldırma ve kaydırma özeti yalnız günceller: kullanıcının kapattığı
        // özet yeniden kurulmaz.
        if (!olustur && etkin.none { it.tag == etiket }) return
        val sirali = satirlar.values.sortedByDescending { it.zaman }
        val ad = hesapAdi?.ifBlank { null } ?: sirali.firstNotNullOfOrNull { it.hesapAdi.ifBlank { null } } ?: ""
        val n = sirali.size
        val baslik = ctx.resources.getQuantityString(R.plurals.bildirim_ozet, n, n)
        val stil = NotificationCompat.InboxStyle().setBigContentTitle(baslik)
        if (ad.isNotBlank()) stil.setSummaryText(ad)
        for (p in sirali.take(SATIR)) stil.addLine(satir(ctx, p))
        val gonderenler = sirali.map { baslikOf(ctx, it) }.distinct().joinToString(", ")
        val b = NotificationCompat.Builder(ctx, KANAL_POSTA)
            .setSmallIcon(R.drawable.ic_bildirim)
            .setColor(vurgu(ctx))
            .setContentTitle(baslik)
            .setContentText(gonderenler)
            .setSubText(ad.ifBlank { null })
            .setStyle(stil)
            .setNumber(n)
            .setWhen(sirali.first().zaman)
            .setShowWhen(true)
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setGroup(GRUP + hesap)
            .setGroupSummary(true)
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            .setSilent(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(acikSurum(ctx, ad, baslik))
            .setContentIntent(uygulamaNiyeti(ctx))
        gonder(ctx, etiket, b.build())
    }

    private fun satir(ctx: Context, p: Posta): CharSequence {
        val kimden = baslikOf(ctx, p)
        val s = SpannableStringBuilder(kimden)
        s.setSpan(StyleSpan(Typeface.BOLD), 0, kimden.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        s.append("  ").append(konuOf(ctx, p))
        return s
    }

    /** Ön plan hizmetinin sessiz, kalıcı bildirimi. */
    fun arkaPlanBildirimi(ctx: Context): Notification =
        NotificationCompat.Builder(ctx, KANAL_ARKA)
            .setSmallIcon(R.drawable.ic_bildirim)
            .setColor(vurgu(ctx))
            .setContentTitle(ctx.getString(R.string.arka_baslik))
            .setContentText(ctx.getString(R.string.arka_metin))
            .setStyle(NotificationCompat.BigTextStyle().bigText(ctx.getString(R.string.arka_metin)))
            .setOngoing(true)
            .setSilent(true)
            .setShowWhen(false)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setContentIntent(uygulamaNiyeti(ctx))
            .addAction(
                NotificationCompat.Action.Builder(
                    R.drawable.ic_kapat,
                    ctx.getString(R.string.arka_kapat),
                    PendingIntent.getBroadcast(
                        ctx,
                        0,
                        Intent(ctx, BildirimAlici::class.java).setAction(EYLEM_ARKA_KAPAT),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                ).setShowsUserInterface(false).build(),
            )
            .build()

    /**
     * Arka plandan hizmet başlatılamadı (Android izin vermedi): yedek
     * yoklamanın çalıştığını ve uygulamayı açınca anlık bağlantının geri
     * geleceğini söyleyen sessiz bildirim. Hizmet başlayınca kalkar.
     */
    fun yedekGoster(ctx: Context) {
        val metin = ctx.getString(R.string.arka_yedek_metin)
        val n = NotificationCompat.Builder(ctx, KANAL_ARKA)
            .setSmallIcon(R.drawable.ic_bildirim)
            .setColor(vurgu(ctx))
            .setContentTitle(ctx.getString(R.string.arka_yedek_baslik))
            .setContentText(metin)
            .setStyle(NotificationCompat.BigTextStyle().bigText(metin))
            .setSilent(true)
            .setShowWhen(false)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setContentIntent(uygulamaNiyeti(ctx))
            .build()
        gonder(ctx, null, n, ID_YEDEK)
    }

    fun yedekKaldir(ctx: Context) {
        NotificationManagerCompat.from(ctx).cancel(ID_YEDEK)
    }

    /** Kilit kanalı: durum çubuğunda görünür ama ses ve titreşim yok. */
    private fun kilitKanali(ctx: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        val kanal = NotificationChannel(
            KANAL_KILIT,
            ctx.getString(R.string.kanal_kilit),
            NotificationManager.IMPORTANCE_DEFAULT,
        ).apply {
            description = ctx.getString(R.string.kanal_kilit_aciklama)
            setShowBadge(false)
            setSound(null, null)
            enableVibration(false)
        }
        nm.createNotificationChannel(kanal)
    }

    /**
     * Parola kilidi açıkken etkinliksiz çekirdek açılamadı (yeniden başlatma,
     * hizmetin yeniden kurulması, yoklama işi): sessiz, kalıcı olmayan bildirim;
     * aynı kimlikle yeniden gösterim yalnız günceller (`setOnlyAlertOnce`).
     * Dokununca uygulama açılır ve kilit ekranı gelir.
     */
    fun kilitliGoster(ctx: Context) {
        kilitKanali(ctx)
        val metin = ctx.getString(R.string.kilit_metin)
        val n = NotificationCompat.Builder(ctx, KANAL_KILIT)
            .setSmallIcon(R.drawable.ic_bildirim)
            .setColor(vurgu(ctx))
            .setContentTitle(ctx.getString(R.string.kilit_baslik))
            .setContentText(metin)
            .setStyle(NotificationCompat.BigTextStyle().bigText(metin))
            .setSilent(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(uygulamaNiyeti(ctx))
            .build()
        gonder(ctx, null, n, ID_KILIT)
    }

    /** Kilit açıldı (pencereli çekirdek başladı). */
    fun kilitliKaldir(ctx: Context) {
        NotificationManagerCompat.from(ctx).cancel(ID_KILIT)
    }

    /** Postayı açan niyet (tek pencere; `singleTask` etkinliğe `onNewIntent`). */
    private fun acmaNiyeti(ctx: Context, p: Posta): PendingIntent {
        val i = Intent(ctx, MainActivity::class.java)
            .setAction(EYLEM_AC)
            .setData(Uri.parse("mailhere-bildirim://posta/${p.id}"))
            .putExtra(E_ID, p.id)
            .putExtra(E_HESAP, p.hesap)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(ctx, 0, i, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    /** Uygulamayı (gelen kutusunda) açar. */
    private fun uygulamaNiyeti(ctx: Context): PendingIntent {
        val i = Intent(ctx, MainActivity::class.java)
            .setAction(EYLEM_UYGULAMA)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(ctx, 0, i, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    /**
     * Alıcıya giden düğme niyeti; posta başına ayrı (veri adresi). Yanıt
     * düğmesi değişkendir: sistem RemoteInput sonucunu niyete yazar (açık
     * bileşenli, yalnız bu uygulamanın alıcısı).
     */
    private fun alici(
        ctx: Context,
        eylem: String,
        p: Posta,
        degisken: Boolean = false,
        ek: (Intent) -> Unit = {},
    ): PendingIntent {
        val i = Intent(ctx, BildirimAlici::class.java)
            .setAction(eylem)
            .setData(Uri.parse("mailhere-bildirim://posta/${p.id}"))
            .putExtras(ekler(p))
        ek(i)
        val bayrak = when {
            !degisken -> PendingIntent.FLAG_IMMUTABLE
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> PendingIntent.FLAG_MUTABLE
            else -> 0
        }
        return PendingIntent.getBroadcast(ctx, 0, i, PendingIntent.FLAG_UPDATE_CURRENT or bayrak)
    }

    private fun etkinler(ctx: Context) = try {
        ctx.getSystemService(NotificationManager::class.java)?.activeNotifications?.toList() ?: emptyList()
    } catch (e: Exception) {
        emptyList()
    }

    /** Postanın etkin bildirimi; kullanıcı kaldırdıysa `null`. */
    private fun bul(ctx: Context, id: Long): Posta? =
        etkinler(ctx).firstOrNull { it.tag == ETIKET_POSTA + id }?.notification?.extras?.let { postaOf(it) }

    /** İzin yoksa (Android 13+) ya da bildirimler kapalıysa sessizce geçer. */
    @SuppressLint("MissingPermission")
    private fun gonder(ctx: Context, etiket: String?, n: Notification, id: Int = 0) {
        val nm = NotificationManagerCompat.from(ctx)
        if (!nm.areNotificationsEnabled()) return
        try {
            nm.notify(etiket, id, n)
        } catch (e: SecurityException) {
            Log.w(TAG, "bildirim izni yok")
        }
    }
}
