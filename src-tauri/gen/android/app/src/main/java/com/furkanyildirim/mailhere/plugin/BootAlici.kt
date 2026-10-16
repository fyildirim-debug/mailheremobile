package com.furkanyildirim.mailhere.plugin

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Telefon yeniden başladı ya da uygulama güncellendi: yoklama işi yeniden
 * zamanlanır ve "Anlık bildirim" açıksa (varsayılan) ön plan hizmeti
 * başlatılır; hizmet etkinlik olmadan etkinliksiz Rust çekirdeğini açar
 * (IMAP IDLE / JMAP akışı, bildirimler, bildirim düğmeleri). Android 12+
 * `BOOT_COMPLETED` ve `MY_PACKAGE_REPLACED` alıcılarının ön plan hizmeti
 * başlatmasına izin verir; Android 15'in açılışta yasakladığı türler
 * (`dataSync`, `mediaPlayback` …) arasında `specialUse` yoktur. İzin
 * verilmezse [ArkaPlan.baslatGerekirse] yedeği (yoklama işi) bırakır ve
 * kullanıcıya sessiz bir bildirimle söyler.
 *
 * "Açılışta başlat" kapalıysa ([ArkaPlan.acilistaBaslar]) telefon açılınca
 * hiçbir şey başlatılmaz: kullanıcı Mailhere'yi açana dek hizmet yok.
 * Güncellemede (`MY_PACKAGE_REPLACED`) ayardan bağımsız olarak önceki durum
 * geri kurulur: sistem süreci güncelleme için kapattı, kullanıcının açık
 * bıraktığı anlık bildirim sürsün.
 */
class BootAlici : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_LOCKED_BOOT_COMPLETED,
            -> if (ArkaPlan.acilistaBaslar(ctx)) baslat(ctx)
            Intent.ACTION_MY_PACKAGE_REPLACED -> baslat(ctx)
        }
    }

    private fun baslat(ctx: Context) {
        ArkaPlanIsi.planla(ctx)
        ArkaPlan.baslatGerekirse(ctx, aciklamaGoster = true)
    }
}
