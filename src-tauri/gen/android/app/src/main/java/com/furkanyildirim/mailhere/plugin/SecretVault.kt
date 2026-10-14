package com.furkanyildirim.mailhere.plugin

import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Parola, OAuth yenileme jetonu ve veritabanı anahtarı kasası.
 *
 * Değerler `AndroidKeyStore` içinde üretilen, dışarı çıkarılamayan AES-256
 * anahtarıyla GCM kipinde şifrelenir (StrongBox varsa onda, yoksa TEE'de).
 * Her değer için Keystore'un ürettiği rastgele 12 baytlık IV kullanılır;
 * ek doğrulanmış veri (AAD) anahtarın adıdır, böylece bir kaydın şifreli
 * metni başka bir ad altına taşınamaz. Şifreli metin `IV || şifre || etiket`
 * olarak Base64 ile `mailhere-kasa` tercih dosyasında (MODE_PRIVATE) durur;
 * dosya yedeklemeye kapalıdır (`allowBackup=false`, veri çıkarma kuralları).
 *
 * Hata metinleri anahtar adı ya da değer içermez.
 */
class SecretVault(context: Context) {
    private val prefs: SharedPreferences =
        context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Kayıt yoksa `null`; çözülemiyorsa (anahtar yenilenmiş, kayıt bozuk) [VaultException]. */
    fun get(name: String): String? {
        val stored = prefs.getString(name, null) ?: return null
        val blob = try {
            Base64.decode(stored, Base64.NO_WRAP)
        } catch (e: IllegalArgumentException) {
            throw VaultException("kasa kaydı bozuk")
        }
        if (blob.size < IV_BYTES + TAG_BYTES) throw VaultException("kasa kaydı bozuk")
        val iv = blob.copyOfRange(0, IV_BYTES)
        val cipherText = blob.copyOfRange(IV_BYTES, blob.size)
        return try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, iv))
            cipher.updateAAD(name.toByteArray(Charsets.UTF_8))
            String(cipher.doFinal(cipherText), Charsets.UTF_8)
        } catch (e: VaultException) {
            throw e
        } catch (e: Exception) {
            // AEADBadTagException, KeyPermanentlyInvalidatedException vb.
            throw VaultException("kasa kaydı çözülemedi")
        }
    }

    fun set(name: String, value: String) {
        val blob = try {
            val cipher = Cipher.getInstance(TRANSFORM)
            // IV'yi Keystore üretir (rastgele 12 bayt); dışarıdan IV verilmez.
            cipher.init(Cipher.ENCRYPT_MODE, key())
            cipher.updateAAD(name.toByteArray(Charsets.UTF_8))
            val iv = cipher.iv
            if (iv.size != IV_BYTES) throw VaultException("beklenmeyen IV uzunluğu")
            iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        } catch (e: VaultException) {
            throw e
        } catch (e: Exception) {
            throw VaultException("kasaya yazılamadı")
        }
        val ok = prefs.edit().putString(name, Base64.encodeToString(blob, Base64.NO_WRAP)).commit()
        if (!ok) throw VaultException("kasa dosyası yazılamadı")
    }

    fun delete(name: String) {
        if (!prefs.contains(name)) return
        if (!prefs.edit().remove(name).commit()) throw VaultException("kasa dosyası yazılamadı")
    }

    /** Kasa anahtarı; yoksa üretilir. Yalnız bu uygulama, kimlik doğrulama şartı yok. */
    private fun key(): SecretKey {
        synchronized(LOCK) {
            val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
            (store.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
            return generate(strongBox = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P)
        }
    }

    private fun generate(strongBox: Boolean): SecretKey {
        val spec = KeyGenParameterSpec.Builder(
            ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(KEY_BITS)
            .setUserAuthenticationRequired(false)
        if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            spec.setIsStrongBoxBacked(true)
        }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        return try {
            generator.init(spec.build())
            generator.generateKey()
        } catch (e: StrongBoxUnavailableException) {
            // Cihazda StrongBox yok: TEE destekli anahtara düşülür.
            if (strongBox) generate(strongBox = false) else throw VaultException("kasa anahtarı üretilemedi")
        } catch (e: Exception) {
            throw VaultException("kasa anahtarı üretilemedi")
        }
    }

    companion object {
        const val PREFS = "mailhere-kasa"
        private const val KEYSTORE = "AndroidKeyStore"
        private const val ALIAS = "mailhere-kasa-anahtari"
        private const val TRANSFORM = "AES/GCM/NoPadding"
        private const val KEY_BITS = 256
        private const val IV_BYTES = 12
        private const val TAG_BITS = 128
        private const val TAG_BYTES = TAG_BITS / 8
        private val LOCK = Any()
    }
}

/** Kasa hatası; iletisi gizli değer taşımaz. */
class VaultException(message: String) : Exception(message)
