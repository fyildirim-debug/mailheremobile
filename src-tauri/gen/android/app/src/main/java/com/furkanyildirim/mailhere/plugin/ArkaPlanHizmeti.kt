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
import android.os.SystemClock
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import app.tauri.plugin.JSObject

/**
 * "Anlık bildirim" (isteğe bağlı): süreci arka planda canlı tutan ön plan
 * hizmeti. Kendi işi yoktur; IMAP IDLE ve JMAP anlık bildirim bağlantıları
 * Rust çekirdeğinde zaten çalışır. Hizmet yalnız şunları sağlar:
 *
 * - Süreç "ön plan hizmeti" önceliğinde kalır: önbellek kipinde dondurulmaz,
 *   bellek baskısında ve üretici uyutucularında sona kalır, Doze'da ağ
 *   kesilmez.
 * - 15 dakikada bir Doze'da da çalışan kesin olmayan alarm ([ArkaPlan.uyan]):
 *   cihaz uyanır, çekirdek bağlantıları bir eşitlemeyle sınar (yarı ölü TCP,
 *   NAT zaman aşımı); yayın kilidi 8 sn tutulur.
 *
 * Tür `specialUse` (Android 14+): `dataSync` Android 15'te 24 saatte 6 saatle
 * sınırlıdır ve süre dolunca hizmet durdurulur; sürekli posta bağlantısına
 * uymaz. `stopWithTask`: görev son uygulamalardan kaydırılınca hizmet durur
 * (etkinlik yok edilince Tauri süreci zaten kapatır; Rust çekirdeği
 * etkinliksiz yaşayamaz). `START_NOT_STICKY`: sistem süreci öldürürse hizmet
 * çekirdeksiz yeniden doğmaz; uygulama açılınca yeniden başlar.
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
            Log.w(TAG, "anlık bildirim hizmeti başlatılamadı")
            stopSelf()
            return START_NOT_STICKY
        }
        ArkaPlan.alarmKur(this)
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        ArkaPlan.alarmKaldir(this)
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

/** Anlık bildirim ayarı (uygulamaya özel tercih dosyası) ve hizmet denetimi. */
object ArkaPlan {
    private const val TERCIH = "mailhere-arka-plan"
    private const val ACIK = "anlik"
    private const val ARALIK = 15 * 60 * 1000L
    private const val UYANIK = 8_000L
    private const val TAG = "Mailhere"

    private fun tercih(ctx: Context) =
        ctx.applicationContext.getSharedPreferences(TERCIH, Context.MODE_PRIVATE)

    fun acik(ctx: Context): Boolean = tercih(ctx).getBoolean(ACIK, false)

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

    /** Açıksa hizmeti başlatır (etkinlik ekrana gelince). */
    fun baslatGerekirse(ctx: Context) {
        if (!acik(ctx)) return
        try {
            baslat(ctx)
        } catch (e: Exception) {
            Log.w(TAG, "anlık bildirim hizmeti başlatılamadı")
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
     * sonraki alarm kurulur. Çekirdek yoksa zincir biter.
     */
    fun uyan(ctx: Context, sonuc: BroadcastReceiver.PendingResult) {
        if (!acik(ctx) || !MhPlugin.ilet(JSObject().put("tur", "uyan"))) {
            sonuc.finish()
            return
        }
        alarmKur(ctx)
        Handler(Looper.getMainLooper()).postDelayed({ sonuc.finish() }, UYANIK)
    }
}
