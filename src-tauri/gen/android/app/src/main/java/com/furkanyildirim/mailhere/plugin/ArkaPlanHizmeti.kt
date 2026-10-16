package com.furkanyildirim.mailhere.plugin

import android.app.AlarmManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Process
import android.os.SystemClock
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * "Anlık bildirim" (varsayılan açık): süreci arka planda canlı tutan ön plan
 * hizmeti ve etkinlik yokken Rust çekirdeğinin sahibi.
 *
 * - Süreç "ön plan hizmeti" önceliğinde kalır: önbellek kipinde dondurulmaz,
 *   bellek baskısında ve üretici uyutucularında sona kalır, Doze'da ağ
 *   kesilmez. IMAP IDLE ve JMAP akışı Rust çekirdeğinde sürer.
 * - Görev son uygulamalardan kaydırılınca durmaz (`stopWithTask=false`):
 *   etkinlik yok edilir ama Rust çıkışı engeller (`lib.rs`), pencereli çekirdek
 *   çalışmaya devam eder, yeni posta bildirilir, düğmeler çalışır.
 * - Süreçte çekirdek yoksa (telefon yeniden başladı, sistem süreci öldürüp
 *   hizmeti yeniden kurdu, bildirim düğmesi hizmeti başlattı) hizmet
 *   etkinliksiz çekirdeği açar ([ArkaKopru.cekirdekBaslat]): aynı eşitleme,
 *   aynı bildirimler, aynı düğmeler; pencere yok. Etkinlik gelince Rust
 *   onu kapatıp pencereli çekirdeği açar (süreç başına tek çekirdek).
 * - 15 dakikada bir Doze'da da çalışan kesin olmayan alarm ([ArkaPlan.uyan]):
 *   çekirdek bağlantıları bir eşitlemeyle sınar (yarı ölü TCP, NAT zaman
 *   aşımı); yayın kilidi 8 sn tutulur.
 *
 * Tür `specialUse` (Android 14+): `dataSync` Android 15'te 24 saatte 6 saatle
 * sınırlıdır ve açılıştan başlatılamaz; sürekli posta bağlantısına uymaz.
 * `START_STICKY`: sistem süreci öldürürse hizmet yeniden doğar ve çekirdeği
 * etkinliksiz açar. Arka plandan başlatma (Android 12+) yalnız izinli
 * yollarla denenir; izin yoksa yoklama işi ([ArkaPlanIsi]) yedektir.
 */
class ArkaPlanHizmeti : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Bildirimler.kanallar(this)
        val bildirim = Bildirimler.arkaPlanBildirimi(this)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                ServiceCompat.startForeground(
                    this,
                    Bildirimler.ID_ARKA,
                    bildirim,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
                )
            } else {
                startForeground(Bildirimler.ID_ARKA, bildirim)
            }
        } catch (e: Exception) {
            Log.w(TAG, "anlık bildirim hizmeti başlatılamadı: ${e.javaClass.simpleName}")
            stopSelf()
            return START_NOT_STICKY
        }
        Bildirimler.yedekKaldir(this)
        ArkaPlan.alarmKur(this)
        // Çekirdek yoksa ve etkinlik de yoksa etkinliksiz açılır (depo açılışı
        // ana iş parçacığını tutmasın). Etkinlik varsa pencereli çekirdek
        // kuruluyordur; kapı (cekirdek.rs) yine de Tauri'ye öncelik verir.
        if (!Yasam.etkinlikVar && !ArkaKopru.cekirdekVar()) {
            Thread {
                val kod = ArkaKopru.baslat()
                Log.i(TAG, "etkinliksiz çekirdek: $kod")
            }.start()
        }
        return START_STICKY
    }

    /**
     * Hizmet durdu (kullanıcı kapattı). Etkinlik yoksa arka plan işi de
     * biter: etkinliksiz çekirdek kapatılır; pencereli çekirdek etkinliksiz
     * yaşıyorsa süreç sonlandırılır (kapalıyken kaydırma zaten çıkardı).
     */
    override fun onDestroy() {
        ArkaPlan.alarmKaldir(this)
        if (!Yasam.etkinlikVar) {
            when (ArkaKopru.durum()) {
                // Kapatma birkaç saniye sürebilir; ana iş parçacığını tutmasın.
                ArkaKopru.DURUM_BASLIKSIZ -> Thread { ArkaKopru.durdur() }.start()
                ArkaKopru.DURUM_TAURI -> {
                    super.onDestroy()
                    Process.killProcess(Process.myPid())
                    return
                }
            }
        }
        super.onDestroy()
    }

    /** Android 15+ süre sınırı (yalnız `dataSync` gibi türlerde gelir). */
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    companion object {
        private const val TAG = "Mailhere"
    }
}

/**
 * Anlık bildirim ve açılışta başlama ayarları (uygulamaya özel tercih
 * dosyası) ve hizmet denetimi.
 */
object ArkaPlan {
    private const val TERCIH = "mailhere-arka-plan"
    private const val ACIK = "anlik"
    private const val ACILIS = "acilis"
    private const val ARALIK = 15 * 60 * 1000L
    private const val UYANIK = 8_000L
    private const val TAG = "Mailhere"

    private fun tercih(ctx: Context) =
        ctx.applicationContext.getSharedPreferences(TERCIH, Context.MODE_PRIVATE)

    /** Kullanıcı hiç seçmediyse açık; kapattıysa kapalı kalır. */
    fun acik(ctx: Context): Boolean = tercih(ctx).getBoolean(ACIK, true)

    /**
     * "Açılışta başlat" (Ayarlar › Genel): telefon açılınca [BootAlici]
     * hizmeti ve yoklama işini kursun mu. Kullanıcı hiç seçmediyse açık.
     * Kapalıyken açılışta hiçbir şey başlatılmaz; uygulama açılınca anlık
     * bildirim ve yoklama her zamanki gibi çalışır.
     */
    fun acilistaBaslar(ctx: Context): Boolean = tercih(ctx).getBoolean(ACILIS, true)

    /** Açılışta başlamayı yazar (eşzamanlı; açılış alıcısı hemen okuyabilsin). */
    fun acilisAyarla(ctx: Context, acik: Boolean): Boolean =
        tercih(ctx).edit().putBoolean(ACILIS, acik).commit()

    /**
     * Ayarı yazar; açıksa hizmeti başlatır. Uygulama ekrandayken çağrılır
     * (Android 12+ arka plandan ön plan hizmeti başlatmaya izin vermez).
     * Başlatılamazsa ayar kapalı kalır ve hata atılır.
     */
    fun ayarla(ctx: Context, acik: Boolean) {
        tercih(ctx).edit().putBoolean(ACIK, acik).apply()
        if (!acik) {
            durdur(ctx)
            return
        }
        try {
            baslat(ctx)
        } catch (e: Exception) {
            tercih(ctx).edit().putBoolean(ACIK, false).apply()
            throw e
        }
    }

    /**
     * Açıksa ve bildirim izni varsa hizmeti başlatır (etkinlik ekrana gelince,
     * açılışta, bildirim düğmesinden, yoklama işinden). Android izin vermezse
     * `false`; [aciklamaGoster] ile kullanıcıya yedek yoklamanın çalıştığı
     * sessiz bir bildirimle söylenir.
     */
    fun baslatGerekirse(ctx: Context, aciklamaGoster: Boolean = false): Boolean {
        if (!acik(ctx)) return false
        if (!NotificationManagerCompat.from(ctx).areNotificationsEnabled()) return false
        return try {
            baslat(ctx)
            true
        } catch (e: Exception) {
            Log.w(TAG, "anlık bildirim hizmeti başlatılamadı: ${e.javaClass.simpleName}")
            if (aciklamaGoster) Bildirimler.yedekGoster(ctx)
            false
        }
    }

    private fun baslat(ctx: Context) {
        ContextCompat.startForegroundService(ctx, Intent(ctx, ArkaPlanHizmeti::class.java))
    }

    private fun durdur(ctx: Context) {
        ctx.stopService(Intent(ctx, ArkaPlanHizmeti::class.java))
        alarmKaldir(ctx)
    }

    private fun uyanNiyeti(ctx: Context): PendingIntent = PendingIntent.getBroadcast(
        ctx,
        0,
        Intent(ctx, BildirimAlici::class.java).setAction(Bildirimler.EYLEM_UYAN),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    /** Kesin olmayan, Doze'da da çalışan alarm (kesin alarm izni gerekmez). */
    fun alarmKur(ctx: Context) {
        val am = ctx.getSystemService(AlarmManager::class.java) ?: return
        am.setAndAllowWhileIdle(
            AlarmManager.ELAPSED_REALTIME_WAKEUP,
            SystemClock.elapsedRealtime() + ARALIK,
            uyanNiyeti(ctx),
        )
    }

    fun alarmKaldir(ctx: Context) {
        ctx.getSystemService(AlarmManager::class.java)?.cancel(uyanNiyeti(ctx))
    }

    /**
     * Alarm geldi: çekirdek çalışıyorsa bağlantıları sınaması istenir ve
     * yayın kilidi kısa süre tutulur (cihaz eşitleme bitmeden uyumasın);
     * sonraki alarm kurulur. Çekirdek yoksa zincir biter; yoklama işi
     * yedektir.
     */
    fun uyan(ctx: Context, sonuc: BroadcastReceiver.PendingResult) {
        if (!acik(ctx) || !ArkaKopru.esitleGuvenli()) {
            sonuc.finish()
            return
        }
        alarmKur(ctx)
        Handler(Looper.getMainLooper()).postDelayed({ sonuc.finish() }, UYANIK)
    }
}
