package com.furkanyildirim.mailhere.plugin

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.util.Log
import androidx.core.app.NotificationManagerCompat

/**
 * Yedek ağ: 15 dakikada bir, ağ varken, yeniden başlatmaya dayanıklı
 * (`setPersisted`) JobScheduler işi. Anlık bildirimin kendisi ön plan
 * hizmeti ve sürekli çekirdektir; bu iş yalnız o yol kopmuşken devreye girer:
 *
 * 1. Süreçte çekirdek varsa (Tauri ya da etkinliksiz) yalnız "şimdi eşitle"
 *    tetikler ve eşitleme bitene dek işin uyanıklığını tutar.
 * 2. Çekirdek yoksa (sistem süreci öldürdü, açılışta hizmet başlatılamadı)
 *    hizmeti başlatmayı dener; başarırsa hizmet uzun süreli çekirdeği açar.
 *    "Açılışta başlat" kapalıysa denemez (iş açılıştan sonra da çalışır;
 *    hizmeti telefon açılınca kendiliğinden kurmasın), 3. adıma geçer.
 * 3. Android arka plandan hizmet başlatmaya izin vermezse (12+; alarm ve iş
 *    muaf değildir) tek seferlik eşitle-bildir yapar: etkinliksiz çekirdek
 *    açılır, tüm hesaplar bir kez eşitlenir, yeni posta aynı bildirim yoluyla
 *    bildirilir, çekirdek kapanır (en çok ~75 sn).
 *
 * Bildirim izni yoksa hiçbir şey yapmadan biter. Hesap yoksa ya da kasada
 * veritabanı anahtarı yoksa Rust tarafı hemen döner (anahtar üretilmez).
 * WorkManager gibi ek kitaplık yok; her şey Rust'ta ve `Throwable` yakalanır.
 */
class ArkaPlanIsi : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        Thread {
            try {
                calis()
            } catch (e: Throwable) {
                Log.w(TAG, "yoklama işi başarısız: ${e.javaClass.simpleName}")
            } finally {
                jobFinished(params, false)
            }
        }.start()
        return true
    }

    /** Koşullar kaybolunca sistem durdurur; süre sınırlı iş kendi biter, yeniden zamanlanır. */
    override fun onStopJob(params: JobParameters): Boolean = true

    private fun calis() {
        if (!NotificationManagerCompat.from(this).areNotificationsEnabled()) return
        if (!ArkaKopru.cekirdekVar() && ArkaPlan.acilistaBaslar(this) && ArkaPlan.baslatGerekirse(this)) {
            // Hizmet çekirdeği uzun süreli açıyor; iş bitti.
            return
        }
        val kod = ArkaKopru.tekSeferGuvenli(SURE)
        Log.i(TAG, "yoklama işi bitti: $kod")
    }

    companion object {
        private const val TAG = "Mailhere"
        private const val IS_ID = 7
        /** JobScheduler'ın en kısa periyodu. */
        private const val ARALIK = 15 * 60 * 1000L
        /** Tek seferlik eşitlemenin üst sınırı (iş en çok 10 dk sürebilir). */
        private const val SURE = 75_000L

        private const val HEMEN_ID = 8

        /**
         * Sunucu itmesi geldi, çekirdek yok: hemen çalışan tek seferlik iş
         * (Android 12+ hızlandırılmış iş; yüksek öncelikli Firebase iletisi
         * buna izin verir). Aynı işi yapar: hizmeti ya da tek seferlik
         * eşitle-bildir'i açar. Bekleyen varsa yenisi eklenmez.
         */
        fun hemen(ctx: Context) {
            val js = ctx.getSystemService(JobScheduler::class.java) ?: return
            try {
                if (js.getPendingJob(HEMEN_ID) != null) return
                val b = JobInfo.Builder(HEMEN_ID, ComponentName(ctx, ArkaPlanIsi::class.java))
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) {
                    b.setExpedited(true)
                }
                js.schedule(b.build())
            } catch (e: Exception) {
                Log.w(TAG, "itme işi zamanlanamadı: ${e.javaClass.simpleName}")
            }
        }

        /** Periyodik işi zamanlar (zaten varsa dokunmaz). */
        fun planla(ctx: Context) {
            val js = ctx.getSystemService(JobScheduler::class.java) ?: return
            try {
                if (js.getPendingJob(IS_ID) != null) return
                val bilgi = JobInfo.Builder(IS_ID, ComponentName(ctx, ArkaPlanIsi::class.java))
                    .setPeriodic(ARALIK)
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    .setPersisted(true)
                    .build()
                js.schedule(bilgi)
            } catch (e: Exception) {
                Log.w(TAG, "yoklama işi zamanlanamadı: ${e.javaClass.simpleName}")
            }
        }
    }
}
