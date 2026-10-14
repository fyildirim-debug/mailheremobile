package com.furkanyildirim.mailhere.plugin

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
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
class KeyArgs {
    lateinit var key: String
}

@InvokeArg
class SetArgs {
    lateinit var key: String
    lateinit var value: String
}

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

/**
 * Mailhere'nin uygulama içi eklentisi; yalnız Rust tarafından çağrılır
 * (`mh-android`, `src/plugin.rs`). Arayüzden erişilen komutu yoktur.
 *
 * - `secretGet` / `secretSet` / `secretDelete`: [SecretVault].
 * - `fileName`: `content://` adresinin görünen adı (ek olarak eklenen dosya).
 * - `openFile` / `shareFile`: önbellekteki (`cache/ekler/`) bir dosyayı
 *   `FileProvider` ile, yalnız o dosyaya özel geçici okuma izniyle açar ya da
 *   paylaşır.
 * - `scanQr`: tam ekran [QrScanActivity]; `{ text: string | null }`.
 * - `systemBars`: durum ve gezinme çubuğu simgelerini uygulamanın temasına uydurur.
 */
@TauriPlugin
class MhPlugin(private val activity: Activity) : Plugin(activity) {
    private val vault by lazy { SecretVault(activity) }

    @Command
    fun secretGet(invoke: Invoke) {
        val args = invoke.parseArgs(KeyArgs::class.java)
        try {
            val value = vault.get(args.key)
            val result = JSObject()
            result.put("value", value ?: JSONObject.NULL)
            invoke.resolve(result)
        } catch (e: VaultException) {
            invoke.reject(e.message ?: "kasa okunamadı")
        }
    }

    @Command
    fun secretSet(invoke: Invoke) {
        val args = invoke.parseArgs(SetArgs::class.java)
        try {
            vault.set(args.key, args.value)
            invoke.resolve()
        } catch (e: VaultException) {
            invoke.reject(e.message ?: "kasaya yazılamadı")
        }
    }

    @Command
    fun secretDelete(invoke: Invoke) {
        val args = invoke.parseArgs(KeyArgs::class.java)
        try {
            vault.delete(args.key)
            invoke.resolve()
        } catch (e: VaultException) {
            invoke.reject(e.message ?: "kasadan silinemedi")
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
