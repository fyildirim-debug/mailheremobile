package com.furkanyildirim.mailhere

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Process
import androidx.core.content.IntentCompat

/**
 * Süreci temiz yeniden başlatan trampolin; ayrı süreçte (`:yeniden`) çalışır,
 * görünmez (saydam tema), son uygulamalarda görünmez.
 *
 * Neden: "Anlık bildirim" açıkken etkinlik yok edilince (son uygulamalardan
 * kaydırma, Android'in arka planda etkinliği yok etmesi) süreç ve Rust
 * çekirdeği yaşamaya devam eder (`lib.rs` çıkışı engeller). Tauri aynı
 * süreçte yeni etkinliğe webview kuramaz (`setup` süreç başına bir kez
 * çalışır; boş ekran). Kullanıcı uygulamayı yeniden açınca [MainActivity]
 * bunu fark eder ([com.furkanyildirim.mailhere.plugin.Yasam.yokEdildi]) ve
 * bu etkinliği başlatır: ana süreç öldürülür, [MainActivity] yeni görevle
 * (dokunulan bildirimin niyeti korunarak) açılır, yeni süreçte çekirdek ve
 * hizmet olağan yoldan başlar. Kullanıcı yalnız kısa bir açılış görür.
 * Kendi süreci sonra kapanır.
 */
class YenidenBaslatActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pid = intent.getIntExtra(E_PID, 0)
        if (pid > 0 && pid != Process.myPid()) Process.killProcess(pid)
        val hedef = Intent(this, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        IntentCompat.getParcelableExtra(intent, E_NIYET, Intent::class.java)?.let { ozgun ->
            // Bildirime dokunuş (POSTA_AC + posta kimliği) yeni süreçte açılır.
            hedef.action = ozgun.action
            hedef.data = ozgun.data
            ozgun.extras?.let { hedef.putExtras(it) }
        }
        startActivity(hedef)
        finish()
        Runtime.getRuntime().exit(0)
    }

    companion object {
        private const val E_PID = "mh.pid"
        private const val E_NIYET = "mh.niyet"

        /** Ana süreçteki (bayat) etkinlikten çağrılır; çağıran ardından `finish()` eder. */
        fun tetikle(etkinlik: Activity) {
            val i = Intent(etkinlik, YenidenBaslatActivity::class.java)
                .putExtra(E_PID, Process.myPid())
                .putExtra(E_NIYET, Intent(etkinlik.intent))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            etkinlik.startActivity(i)
        }
    }
}
