package com.furkanyildirim.mailhere

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.util.Log
import com.furkanyildirim.mailhere.plugin.YazmaNiyeti

/**
 * "Yeni posta" uygulama kısayolunun (`res/xml/kisayollar.xml`; başlatıcıda
 * simgeye uzun basış) trampolini: görünmez, kendi boş görevinde açılır
 * (`taskAffinity=""`), isteği [MainActivity]'ye iletir ve kapanır.
 *
 * Neden: Android statik kısayolun niyetine `FLAG_ACTIVITY_NEW_TASK` ve
 * `FLAG_ACTIVITY_CLEAR_TASK` ekler; hedef [MainActivity] olsaydı açık etkinlik
 * her kısayolda yok edilirdi. Tauri aynı süreçte ikinci bir webview kuramaz
 * (bkz. [YenidenBaslatActivity]) ve yazılmakta olan posta kaybolurdu. Buradan
 * giden niyet yalnız `NEW_TASK` taşır: açık etkinlik `onNewIntent` alır,
 * etkinlik yoksa olağan yoldan açılır. Yazma ekranını `MhPlugin` açar
 * ([YazmaNiyeti]).
 *
 * Dışa kapalıdır: kısayolu sistem, yayımlayan uygulamanın kimliğiyle başlatır.
 */
class KisayolActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Yeniden kurulan (kayıtlı durumlu) etkinlik isteği yinelemez.
        if (savedInstanceState == null && intent?.action == YazmaNiyeti.EYLEM_YENI_POSTA) {
            try {
                startActivity(
                    Intent(this, MainActivity::class.java)
                        .setAction(YazmaNiyeti.EYLEM_YENI_POSTA)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                )
            } catch (e: Exception) {
                Log.w(TAG, "kısayol açılamadı: ${e.javaClass.simpleName}")
            }
        }
        finish()
    }

    private companion object {
        const val TAG = "Mailhere"
    }
}
