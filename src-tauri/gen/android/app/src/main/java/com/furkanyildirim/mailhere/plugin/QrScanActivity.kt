package com.furkanyildirim.mailhere.plugin

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.os.Bundle
import android.util.AttributeSet
import android.util.Size
import android.view.View
import android.view.ViewGroup
import android.widget.ImageButton
import android.widget.TextView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.updateLayoutParams
import androidx.core.view.updatePadding
import com.furkanyildirim.mailhere.R
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.LuminanceSource
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Tam ekran QR tarayıcı: bilgisayardaki aktarım kodunu (`mailhere://aktar?…`)
 * kameradan okur. Görüntü cihazda kalır; ML Kit ya da Google Play Hizmetleri
 * kullanılmaz. Önizleme ve kare analizi CameraX, çözme zxing core (ikisi
 * Apache-2.0).
 *
 * Çözme yolu: her karenin Y (parlaklık) düzlemi sıkışık diziye alınır; önce
 * çerçeve kılavuzuna düşen orta kare kesilip sensör dönüşüne göre dik
 * çevrilir, bulunamazsa tüm kare denenir; her ikisi düz ve ters renkle.
 * Yalnız aktarım öneki olan metin kabul edilir; başka QR'da kısa uyarı
 * gösterilir ve tarama sürer. Bulununca titreşim yok: sonuç döner, ekran
 * kapanır.
 *
 * Sonuç: `RESULT_OK` + [EXTRA_METIN]; vazgeçince `RESULT_CANCELED`; kamera
 * izni verilmedi ya da kamera açılamadıysa `RESULT_CANCELED` + [EXTRA_HATA]
 * ([HATA_IZIN] | [HATA_KAMERA]). Yalnız [MhPlugin] (`scanQr`) açar; dışa
 * açık değildir (manifest `exported=false`).
 */
class QrScanActivity : AppCompatActivity() {
    private lateinit var onizleme: PreviewView
    private lateinit var fener: ImageButton
    private lateinit var uyari: TextView
    private var kamera: Camera? = null
    private var fenerAcik = false
    private val cozucu: ExecutorService = Executors.newSingleThreadExecutor()
    private val okuyucu = MultiFormatReader().apply {
        setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE)))
    }

    @Volatile
    private var bitti = false
    private val uyariyiGizle = Runnable { uyari.visibility = View.GONE }

    /** Kamera izni çalışma anında, yalnız bu ekranda istenir. */
    private val izinIstegi = registerForActivityResult(ActivityResultContracts.RequestPermission()) { verildi ->
        if (verildi) kamerayiAc() else bitir(null, HATA_IZIN)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Ekran her temada koyudur; sistem çubukları saydam, simgeleri açık renk.
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        setContentView(R.layout.qr_tara)
        onizleme = findViewById(R.id.onizleme)
        fener = findViewById(R.id.fener)
        uyari = findViewById(R.id.uyari)
        val ustCubuk = findViewById<View>(R.id.ust_cubuk)
        val uyariAltBosluk = uyari.marginBottomPx()
        ViewCompat.setOnApplyWindowInsetsListener(findViewById(R.id.kok)) { _, insets ->
            val kenarlar = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
            )
            ustCubuk.updatePadding(left = kenarlar.left, top = kenarlar.top, right = kenarlar.right)
            uyari.updateLayoutParams<ViewGroup.MarginLayoutParams> {
                bottomMargin = uyariAltBosluk + kenarlar.bottom
            }
            insets
        }
        findViewById<View>(R.id.geri).setOnClickListener { bitir(null, null) }
        fener.setOnClickListener { feneriDegistir() }

        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            kamerayiAc()
        } else {
            izinIstegi.launch(Manifest.permission.CAMERA)
        }
    }

    override fun onDestroy() {
        uyari.removeCallbacks(uyariyiGizle)
        cozucu.shutdown()
        super.onDestroy()
    }

    /** Arka kamera; önizleme + kare analizi (en son kare, ~720p). */
    private fun kamerayiAc() {
        val saglayici = ProcessCameraProvider.getInstance(this)
        saglayici.addListener({
            try {
                val provider = saglayici.get()
                val preview = Preview.Builder().build()
                preview.setSurfaceProvider(onizleme.surfaceProvider)
                val analiz = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .setResolutionSelector(
                        ResolutionSelector.Builder()
                            .setResolutionStrategy(
                                ResolutionStrategy(
                                    Size(1280, 720),
                                    ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER
                                )
                            )
                            .build()
                    )
                    .build()
                analiz.setAnalyzer(cozucu) { kare -> kareyiIsle(kare) }
                provider.unbindAll()
                val k = provider.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, preview, analiz)
                kamera = k
                fener.visibility = if (k.cameraInfo.hasFlashUnit()) View.VISIBLE else View.GONE
            } catch (e: Exception) {
                // Kamera yok, başka uygulama tutuyor ya da sağlayıcı açılamadı.
                bitir(null, HATA_KAMERA)
            }
        }, ContextCompat.getMainExecutor(this))
    }

    private fun kareyiIsle(kare: ImageProxy) {
        kare.use {
            if (bitti) return
            val metin = coz(it) ?: return
            if (!metin.startsWith(ONEK)) {
                runOnUiThread { uyar() }
                return
            }
            bitti = true
            runOnUiThread { bitir(metin, null) }
        }
    }

    /**
     * Önce çerçevedeki orta kare (dik çevrilmiş), sonra tüm kare; her ikisi
     * düz ve ters renkle.
     */
    private fun coz(kare: ImageProxy): String? {
        val genislik = kare.width
        val yukseklik = kare.height
        val parlaklik = yDuzlemi(kare)
        val kenar = (minOf(genislik, yukseklik) * CERCEVE_ORANI).toInt()
        val sol = (genislik - kenar) / 2
        val ust = (yukseklik - kenar) / 2
        val orta = dondur(kes(parlaklik, genislik, sol, ust, kenar), kenar, kare.imageInfo.rotationDegrees)
        return oku(PlanarYUVLuminanceSource(orta, kenar, kenar, 0, 0, kenar, kenar, false))
            ?: oku(PlanarYUVLuminanceSource(parlaklik, genislik, yukseklik, 0, 0, genislik, yukseklik, false))
    }

    private fun oku(kaynak: LuminanceSource): String? = okuTek(kaynak) ?: okuTek(kaynak.invert())

    private fun okuTek(kaynak: LuminanceSource): String? = try {
        okuyucu.decodeWithState(BinaryBitmap(HybridBinarizer(kaynak))).text
    } catch (e: ReaderException) {
        null
    } finally {
        okuyucu.reset()
    }

    /** Y düzlemini satır ve piksel adımını açarak `genişlik × yükseklik` diziye alır. */
    private fun yDuzlemi(kare: ImageProxy): ByteArray {
        val duzlem = kare.planes[0]
        val genislik = kare.width
        val yukseklik = kare.height
        val tampon = duzlem.buffer
        val satirAdimi = duzlem.rowStride
        val pikselAdimi = duzlem.pixelStride
        val out = ByteArray(genislik * yukseklik)
        tampon.rewind()
        if (pikselAdimi == 1 && satirAdimi == genislik) {
            tampon.get(out, 0, minOf(tampon.remaining(), out.size))
            return out
        }
        val satir = ByteArray(satirAdimi)
        for (s in 0 until yukseklik) {
            tampon.position(s * satirAdimi)
            tampon.get(satir, 0, minOf(satirAdimi, tampon.remaining()))
            if (pikselAdimi == 1) {
                System.arraycopy(satir, 0, out, s * genislik, genislik)
            } else {
                for (p in 0 until genislik) out[s * genislik + p] = satir[p * pikselAdimi]
            }
        }
        return out
    }

    private fun kes(veri: ByteArray, genislik: Int, sol: Int, ust: Int, kenar: Int): ByteArray {
        val out = ByteArray(kenar * kenar)
        for (s in 0 until kenar) System.arraycopy(veri, (ust + s) * genislik + sol, out, s * kenar, kenar)
        return out
    }

    /** Kare görüntüyü sensör dönüşü kadar saat yönünde çevirir (0 / 90 / 180 / 270). */
    private fun dondur(veri: ByteArray, kenar: Int, derece: Int): ByteArray {
        val son = kenar - 1
        val out = ByteArray(veri.size)
        when (derece) {
            90 -> for (y in 0 until kenar) for (x in 0 until kenar) out[x * kenar + (son - y)] = veri[y * kenar + x]
            180 -> for (y in 0 until kenar) for (x in 0 until kenar) out[(son - y) * kenar + (son - x)] = veri[y * kenar + x]
            270 -> for (y in 0 until kenar) for (x in 0 until kenar) out[(son - x) * kenar + y] = veri[y * kenar + x]
            else -> return veri
        }
        return out
    }

    /** Başka bir QR okundu: kısa uyarı, tarama sürer. */
    private fun uyar() {
        if (bitti || isFinishing) return
        uyari.setText(R.string.qr_wrong)
        uyari.visibility = View.VISIBLE
        uyari.removeCallbacks(uyariyiGizle)
        uyari.postDelayed(uyariyiGizle, UYARI_MS)
    }

    private fun feneriDegistir() {
        val k = kamera ?: return
        fenerAcik = !fenerAcik
        k.cameraControl.enableTorch(fenerAcik)
        val renk = ContextCompat.getColor(this, if (fenerAcik) R.color.qr_vurgu else R.color.qr_murekkep)
        fener.imageTintList = ColorStateList.valueOf(renk)
        fener.contentDescription = getString(if (fenerAcik) R.string.qr_torch_off else R.string.qr_torch_on)
    }

    private fun bitir(metin: String?, hata: String?) {
        if (isFinishing) return
        when {
            metin != null -> setResult(RESULT_OK, Intent().putExtra(EXTRA_METIN, metin))
            hata != null -> setResult(RESULT_CANCELED, Intent().putExtra(EXTRA_HATA, hata))
            else -> setResult(RESULT_CANCELED)
        }
        finish()
    }

    private fun View.marginBottomPx(): Int = (layoutParams as? ViewGroup.MarginLayoutParams)?.bottomMargin ?: 0

    companion object {
        const val EXTRA_METIN = "metin"
        const val EXTRA_HATA = "hata"
        const val HATA_IZIN = "izin"
        const val HATA_KAMERA = "kamera"

        /** Yalnız bu önekle başlayan kod kabul edilir (`crates/mh-transfer/src/code.rs`). */
        private const val ONEK = "mailhere://aktar?"

        /** Çerçeve kılavuzunun kısa kenara oranı ([QrCerceveView] ile aynı). */
        const val CERCEVE_ORANI = 0.68f
        private const val UYARI_MS = 1800L
    }
}

/**
 * Kamera üstünde çerçeve kılavuzu: dışarısı karartılır, ortadaki kare vurgu
 * renkli ince çizgiyle çevrelenir (köşe 16 dp). Ölçü [QrScanActivity.CERCEVE_ORANI].
 */
class QrCerceveView @JvmOverloads constructor(context: Context, attrs: AttributeSet? = null) : View(context, attrs) {
    private val ortu = Paint().apply { color = ContextCompat.getColor(context, R.color.qr_ortu) }
    private val cizgi = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = dp(2f)
        color = ContextCompat.getColor(context, R.color.qr_vurgu)
    }
    private val yol = Path().apply { fillType = Path.FillType.EVEN_ODD }
    private val kare = RectF()

    private fun dp(deger: Float) = deger * resources.displayMetrics.density

    override fun onDraw(canvas: Canvas) {
        val g = width.toFloat()
        val y = height.toFloat()
        val kenar = minOf(g, y) * QrScanActivity.CERCEVE_ORANI
        kare.set((g - kenar) / 2, (y - kenar) / 2, (g + kenar) / 2, (y + kenar) / 2)
        val kose = dp(16f)
        yol.reset()
        yol.addRect(0f, 0f, g, y, Path.Direction.CW)
        yol.addRoundRect(kare, kose, kose, Path.Direction.CW)
        canvas.drawPath(yol, ortu)
        canvas.drawRoundRect(kare, kose, kose, cizgi)
    }
}
