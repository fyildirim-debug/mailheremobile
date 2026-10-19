package com.furkanyildirim.mailhere

import android.app.ActivityManager
import android.app.Application
import android.os.Build
import android.os.Process
import android.util.Log
import com.furkanyildirim.mailhere.plugin.ArkaKopru
import com.furkanyildirim.mailhere.plugin.ArkaPlanIsi
import com.furkanyildirim.mailhere.plugin.Bildirimler
import com.furkanyildirim.mailhere.plugin.Itme
import java.io.File

/**
 * Süreç başlangıcı (etkinlikle ya da etkinliksiz: açılış alıcısı, hizmet,
 * JobScheduler, bildirim düğmesi). Yerel kitaplığı yükler ve etkinliksiz JNI
 * köprüsünü kurar ([ArkaKopru.kur]: uygulama `Context`i, veri ve önbellek
 * klasörleri, günlük, çalışma ortamı), bildirim kanallarını oluşturur,
 * yoklama işini zamanlar ve sunucu itmesini (Firebase) kurar. Trampolin sürecinde (`:yeniden`) hiçbiri yapılmaz:
 * o süreç yalnız ana süreci öldürüp uygulamayı yeniden açar.
 */
class MailhereApp : Application() {
    override fun onCreate() {
        super.onCreate()
        if (surecAdi().endsWith(YENIDEN_SUREC)) return
        Bildirimler.kanallar(this)
        if (ArkaKopru.yukle()) {
            try {
                ArkaKopru.kur(this, filesDir.absolutePath, cacheDir.absolutePath)
            } catch (e: Throwable) {
                Log.e(TAG, "JNI köprüsü kurulamadı: ${e.javaClass.simpleName}")
            }
        }
        ArkaPlanIsi.planla(this)
        // Sunucu itmesi: Firebase ayarı aktarıcıdan, jeton kasaya (Itme.kt).
        Itme.kur(this)
    }

    private fun surecAdi(): String {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) return getProcessName()
        try {
            val pid = Process.myPid()
            getSystemService(ActivityManager::class.java)?.runningAppProcesses
                ?.firstOrNull { it.pid == pid }?.processName?.let { return it }
            return File("/proc/self/cmdline").readText().trim { it <= ' ' || it == '\u0000' }
        } catch (e: Exception) {
            return packageName
        }
    }

    companion object {
        private const val TAG = "Mailhere"

        /** Trampolin etkinliğinin süreç eki (`AndroidManifest.xml` → `android:process`). */
        const val YENIDEN_SUREC = ":yeniden"
    }
}
