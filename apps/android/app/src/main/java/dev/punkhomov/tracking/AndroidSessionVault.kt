package dev.punkhomov.tracking

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import dev.punkhomov.tracking.nativecore.SessionVault
import dev.punkhomov.tracking.nativecore.TrackerException
import java.io.File
import java.io.FileOutputStream
import java.nio.file.Files
import java.nio.file.NoSuchFileException
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import java.nio.file.StandardCopyOption.REPLACE_EXISTING
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

internal class AndroidSessionVault(file: File) : SessionVault {
    private val path = file.toPath()
    private val temporary = path.resolveSibling("${file.name}.new")
    private val alias = "Tracking.Supabase.${file.name}"

    @Synchronized
    override fun load(): ByteArray? = storageOperation("прочитать") {
        val encrypted = try {
            Files.readAllBytes(path)
        } catch (_: NoSuchFileException) {
            return@storageOperation null
        }
        val key = keyStore().getKey(alias, null) as? SecretKey
        if (key == null) {
            Files.deleteIfExists(path)
            return@storageOperation null
        }
        require(encrypted.size >= 1 + NONCE_BYTES + TAG_BYTES && encrypted[0] == VERSION)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BYTES * 8, encrypted, 1, NONCE_BYTES))
        cipher.updateAAD(byteArrayOf(VERSION))
        cipher.doFinal(encrypted, 1 + NONCE_BYTES, encrypted.size - 1 - NONCE_BYTES)
    }

    @Synchronized
    override fun save(session: ByteArray) = storageOperation("сохранить") {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, encryptionKey())
        require(cipher.iv.size == NONCE_BYTES)
        cipher.updateAAD(byteArrayOf(VERSION))
        val encrypted = cipher.doFinal(session)
        try {
            FileOutputStream(temporary.toFile()).use { file ->
                file.write(VERSION.toInt())
                file.write(cipher.iv)
                file.write(encrypted)
                file.fd.sync()
            }
            Files.move(temporary, path, ATOMIC_MOVE, REPLACE_EXISTING)
        } finally {
            Files.deleteIfExists(temporary)
        }
        Unit
    }

    @Synchronized
    override fun clear() {
        storageOperation("удалить") {
            Files.deleteIfExists(temporary)
            Files.deleteIfExists(path)
        }
    }

    private fun encryptionKey(): SecretKey {
        val existing = keyStore().getKey(alias, null) as? SecretKey
        if (existing != null) return existing
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE).apply {
            init(
                KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setKeySize(256)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
        }.generateKey()
    }

    private fun keyStore() = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    private inline fun <T> storageOperation(action: String, operation: () -> T): T = try {
        operation()
    } catch (_: Exception) {
        throw TrackerException.Operation("Не удалось $action сохранённую сессию Supabase в Android Keystore.")
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val VERSION: Byte = 1
        const val NONCE_BYTES = 12
        const val TAG_BYTES = 16
    }
}
