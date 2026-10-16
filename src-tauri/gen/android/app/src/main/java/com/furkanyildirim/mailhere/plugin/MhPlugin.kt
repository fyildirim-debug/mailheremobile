package com.furkanyildirim.mailhere.plugin

import android.app.Activity
import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.OpenableColumns
import android.provider.Settings
import android.view.HapticFeedbackConstants
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import java.io.File

@InvokeArg
class UriArgs {
    lateinit var uri: String
}

@InvokeArg
class BarsArgs {
    var dark: Boolean = false
}

@InvokeArg
class FileArgs {
    lateinit var path: String
    var mime: String? = null
}

@InvokeArg
class ModArgs {
    var acik: Boolean = false
}

@InvokeArg
class HaptikArgs {
    var kind: String = ""
}

/**
 * Mailhere'nin uygulama içi eklentisi; yalnız Rust tarafından, **pencere
 * varken** çağrılır (`mh-android`, `src/plugin.rs`). Arayüzden erişilen
 * komutu yoktur. Buradaki her komut arayüzden gelen bir isteğin sonucudur
 * (dosya, QR, çubuklar, pil, anlık bildirim anahtarı, dokunsal geri bildirim,
 * bildirim ayarları sayfası); çekirdeğin kendi başlattığı işler (kasa, yeni
 * posta bildirimi, bildirim eylemleri) etkinlik gerektirmeyen JNI köprüsünden
 * geçer ([ArkaKopru]), çünkü Tauri'nin eklenti köprüsü etkinlik yok
 * edilmişken panikler.
 *
 * - `fileName`: `content://` adresinin görünen adı (ek olarak eklenen dosya).
 * - `openFile` / `shareFile`: önbellekteki (`cache/ekler/`) bir dosyayı
 *   `FileProvider` ile, yalnız o dosyaya özel geçici okuma izniyle açar ya da
 *   paylaşır.
 * - `scanQr`: tam ekran [QrScanActivity]; `{ text: string | null }`.
 * - `systemBars`: durum ve gezinme çubuğu simgelerini uygulamanın temasına uydurur.
 * - `openNotificationSettings`: Mailhere'nin sistem bildirim ayarları.
 * - Bildirim sağlığı: `pilDurumu`, `pilAyarlari`, `arkaPlanDurumu`,
 *   `arkaPlanAyarla` ([ArkaPlan]).
 * - Açılışta başlama: `acilisDurumu`, `acilisAyarla` ([ArkaPlan.acilistaBaslar]).
 * - `haptic`: dokunsal geri bildirim (sistem ayarına uyar).
 *
 * Bildirime dokunuş etkinliğe gelir ([bildirimdenAc]) ve `ArkaKopru.eylem`
 * ile Rust'a `ac` eylemi gider; çekirdek henüz dinlemiyorsa (soğuk açılış)
 * [Yasam] saklar, çekirdek açılınca `ArkaKopru.bekleyenler` ile alır.
 */
@TauriPlugin
class MhPlugin(private val activity: Activity) : Plugin(activity) {
    private val ctx: Context = activity.applicationContext

    init {
        Bildirimler.kanallar(ctx)
        bildirimdenAc(activity.intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        bildirimdenAc(intent)
    }

    /** Bildirime dokunuldu: bildirim kalkar, posta açılır (Rust `mh:open`). */
    private fun bildirimdenAc(intent: Intent?) {
        if (intent?.action != Bildirimler.EYLEM_AC) return
        val id = intent.getLongExtra(Bildirimler.E_ID, -1)
        if (id < 0) return
        // Aynı niyet (ör. yapılandırma değişikliği) ikinci kez işlenmesin.
        intent.action = null
        Bildirimler.dokunuldu(ctx, id, intent.getStringExtra(Bildirimler.E_HESAP))
        if (!ArkaKopru.eylemGonder(JSONObject().put("tur", "ac").put("id", id))) {
            Yasam.acilacakYaz(id)
        }
    }

    /** Seçilen belgenin görünen adı (`OpenableColumns.DISPLAY_NAME`); yoksa `null`. */
    @Command
    fun fileName(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        val result = JSObject()
        var name: String? = null
        try {
            val uri = Uri.parse(args.uri)
            activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                val col = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (col >= 0 && c.moveToFirst() && !c.isNull(col)) name = c.getString(col)
            }
            if (name == null) name = uri.lastPathSegment
        } catch (e: Exception) {
            // Ad bulunamazsa Rust tarafı varsayılan ad kullanır.
        }
        result.put("name", name ?: JSONObject.NULL)
        invoke.resolve(result)
    }

    /** Dosyayı türüne uygun uygulamada açar (`ACTION_VIEW`). */
    @Command
    fun openFile(invoke: Invoke) {
        val args = invoke.parseArgs(FileArgs::class.java)
        try {
            val uri = providerUri(args.path)
            val intent = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, mimeOf(args.mime))
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            start(Intent.createChooser(intent, null))
            invoke.resolve()
        } catch (e: ActivityNotFoundException) {
            invoke.reject("Bu dosya türünü açabilecek uygulama yok")
        } catch (e: Exception) {
            invoke.reject(e.message ?: "dosya açılamadı")
        }
    }

    /** Dosyayı paylaşım sayfasına verir (`ACTION_SEND`). */
    @Command
    fun shareFile(invoke: Invoke) {
        val args = invoke.parseArgs(FileArgs::class.java)
        try {
            val uri = providerUri(args.path)
            val intent = Intent(Intent.ACTION_SEND)
                .setType(mimeOf(args.mime))
                .putExtra(Intent.EXTRA_STREAM, uri)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            start(Intent.createChooser(intent, null))
            invoke.resolve()
        } catch (e: ActivityNotFoundException) {
            invoke.reject("Paylaşabilecek uygulama yok")
        } catch (e: Exception) {
            invoke.reject(e.message ?: "dosya paylaşılamadı")
        }
    }

    /**
     * Tam ekran QR tarayıcıyı açar; sonuç [qrSonuc] ile `{ text: string | null }`
     * olarak döner (vazgeçince `null`). Kamera izni verilmediyse `kamera-izni`,
     * kamera açılamadıysa `kamera` iletisiyle reddedilir; Rust tarafı
     * (`src/qr.rs`) kullanıcı metnine çevirir.
     */
    @Command
    fun scanQr(invoke: Invoke) {
        // Etkinlik başlatma ana iş parçacığında; çağrı Rust'tan başka bir iş parçacığıyla gelir.
        activity.runOnUiThread {
            try {
                startActivityForResult(invoke, Intent(activity, QrScanActivity::class.java), "qrSonuc")
            } catch (e: Exception) {
                invoke.reject(HATA_KAMERA)
            }
        }
    }

    @ActivityCallback
    fun qrSonuc(invoke: Invoke, result: ActivityResult) {
        val veri = result.data
        val sonuc = JSObject()
        if (result.resultCode == Activity.RESULT_OK) {
            val metin = veri?.getStringExtra(QrScanActivity.EXTRA_METIN)
            sonuc.put("text", if (metin.isNullOrEmpty()) JSONObject.NULL else metin)
            invoke.resolve(sonuc)
            return
        }
        when (veri?.getStringExtra(QrScanActivity.EXTRA_HATA)) {
            QrScanActivity.HATA_IZIN -> invoke.reject(HATA_IZIN)
            QrScanActivity.HATA_KAMERA -> invoke.reject(HATA_KAMERA)
            else -> {
                sonuc.put("text", JSONObject.NULL)
                invoke.resolve(sonuc)
            }
        }
    }

    /**
     * Koyu temada açık, açık temada koyu simgeler. Tema dosyası yalnız
     * telefonun gece kipini bilir; kullanıcı uygulamada başka tema seçebilir.
     */
    @Command
    fun systemBars(invoke: Invoke) {
        val args = invoke.parseArgs(BarsArgs::class.java)
        activity.runOnUiThread {
            val bars = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
            bars.isAppearanceLightStatusBars = !args.dark
            bars.isAppearanceLightNavigationBars = !args.dark
        }
        invoke.resolve()
    }

    @Command
    fun openNotificationSettings(invoke: Invoke) {
        val intent = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                .putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName)
        } else {
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                .setData(Uri.fromParts("package", activity.packageName, null))
        }
        try {
            start(intent)
            invoke.resolve()
        } catch (e: ActivityNotFoundException) {
            invoke.reject("Bildirim ayarları açılamadı")
        }
    }

    /**
     * Pil kısıtlaması: `unrestricted` (optimizasyondan muaf), `optimized`
     * (varsayılan) ya da `restricted` (kullanıcı "Kısıtlı" seçmiş; Android 9+).
     */
    @Command
    fun pilDurumu(invoke: Invoke) {
        val result = JSObject()
        result.put("durum", pilKisiti())
        invoke.resolve(result)
    }

    private fun pilKisiti(): String {
        val am = ctx.getSystemService(ActivityManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P && am?.isBackgroundRestricted == true) {
            return "restricted"
        }
        val pm = ctx.getSystemService(PowerManager::class.java)
        return if (pm?.isIgnoringBatteryOptimizations(ctx.packageName) == true) "unrestricted" else "optimized"
    }

    /**
     * Kısıtlamayı kaldırma: "Kısıtlı"ysa uygulamanın ayar sayfası (pil
     * kullanımı oradan "Kısıtlamasız" yapılır), değilse sistemin "pil
     * optimizasyonunu yok say" isteği; açılamazsa optimizasyon listesi.
     * Kullanıcı dönünce çözülür.
     */
    @Command
    fun pilAyarlari(invoke: Invoke) {
        val paket = Uri.fromParts("package", ctx.packageName, null)
        val adaylar = if (pilKisiti() == "optimized") {
            listOf(
                Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, paket),
                Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, paket),
            )
        } else {
            listOf(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, paket))
        }
        activity.runOnUiThread {
            for (intent in adaylar) {
                try {
                    startActivityForResult(invoke, intent, "pilSonuc")
                    return@runOnUiThread
                } catch (e: ActivityNotFoundException) {
                    // Sıradaki sayfa denenir.
                } catch (e: SecurityException) {
                    // Sıradaki sayfa denenir.
                }
            }
            invoke.reject("Pil ayarları açılamadı")
        }
    }

    @ActivityCallback
    fun pilSonuc(invoke: Invoke, result: ActivityResult) {
        invoke.resolve()
    }

    /** Anlık bildirim açık mı: `{ acik: boolean }` (seçilmemişse açık). */
    @Command
    fun arkaPlanDurumu(invoke: Invoke) {
        val result = JSObject()
        result.put("acik", ArkaPlan.acik(ctx))
        invoke.resolve(result)
    }

    /** Anlık bildirimi açar/kapatır; açılışta hizmet hemen başlar. */
    @Command
    fun arkaPlanAyarla(invoke: Invoke) {
        val args = invoke.parseArgs(ModArgs::class.java)
        try {
            ArkaPlan.ayarla(ctx, args.acik)
            val result = JSObject()
            result.put("acik", ArkaPlan.acik(ctx))
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("anlık bildirim başlatılamadı")
        }
    }

    /** Açılışta başlama açık mı: `{ acik: boolean }` (seçilmemişse açık). */
    @Command
    fun acilisDurumu(invoke: Invoke) {
        val result = JSObject()
        result.put("acik", ArkaPlan.acilistaBaslar(ctx))
        invoke.resolve(result)
    }

    /** Açılışta başlamayı açar/kapatır (yalnız ayar; hizmete dokunmaz). */
    @Command
    fun acilisAyarla(invoke: Invoke) {
        val args = invoke.parseArgs(ModArgs::class.java)
        if (!ArkaPlan.acilisAyarla(ctx, args.acik)) {
            invoke.reject("açılış ayarı yazılamadı")
            return
        }
        val result = JSObject()
        result.put("acik", ArkaPlan.acilistaBaslar(ctx))
        invoke.resolve(result)
    }

    /**
     * Dokunsal geri bildirim: `tick` (kaydırma eşiği), `confirm`, `reject`.
     * Sistem "dokunma geri bildirimi" ayarına uyar (bayrak verilmez).
     */
    @Command
    fun haptic(invoke: Invoke) {
        val args = invoke.parseArgs(HaptikArgs::class.java)
        val sabit = when (args.kind) {
            "tick" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                HapticFeedbackConstants.GESTURE_THRESHOLD_ACTIVATE
            } else {
                HapticFeedbackConstants.CLOCK_TICK
            }
            "confirm" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                HapticFeedbackConstants.CONFIRM
            } else {
                HapticFeedbackConstants.VIRTUAL_KEY
            }
            "reject" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                HapticFeedbackConstants.REJECT
            } else {
                HapticFeedbackConstants.LONG_PRESS
            }
            else -> {
                invoke.reject("bilinmeyen dokunuş türü")
                return
            }
        }
        activity.runOnUiThread {
            activity.window?.decorView?.performHapticFeedback(sabit)
        }
        invoke.resolve()
    }

    private fun start(intent: Intent) {
        activity.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun mimeOf(mime: String?): String =
        if (mime.isNullOrBlank() || !mime.contains('/')) "application/octet-stream" else mime

    /**
     * Yalnız önbelleğin `ekler/` klasöründeki dosyalar paylaşılır
     * (`res/xml/ekler_yollari.xml`); başka bir yol reddedilir.
     */
    private fun providerUri(path: String): Uri {
        val file = File(path).canonicalFile
        val root = File(activity.cacheDir, EKLER).canonicalFile
        if (!file.isFile || !file.path.startsWith(root.path + File.separator)) {
            throw IllegalArgumentException("dosya paylaşılabilir klasörde değil")
        }
        return FileProvider.getUriForFile(activity, "${activity.packageName}.ekler", file)
    }

    companion object {
        const val EKLER = "ekler"

        /** `scanQr` ret iletileri (`src/qr.rs` ile aynı). */
        const val HATA_IZIN = "kamera-izni"
        const val HATA_KAMERA = "kamera"
    }
}
