package dev.punkhomov.tracking

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.punkhomov.tracking.nativecore.Tracker
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SessionSmokeTest {
    private val directory = InstrumentationRegistry.getInstrumentation()
        .targetContext.noBackupFilesDir.resolve("session-smoke").apply { mkdirs() }
    private val encrypted = directory.resolve("smoke-vault.session")
    private val vault = AndroidSessionVault(encrypted)

    @Test
    fun persistSession() {
        Tracker.openWithSessionStorage(directory.resolve("tracking.sqlite3").absolutePath, vault).use { tracker ->
            tracker.configureSync(PROJECT, "sb_publishable_fixture")
            vault.save(
                """{
                    "account": {"project": "$PROJECT", "user": "6abe3098-74e4-4d21-8337-19b48e60dc3c"},
                    "refresh_token": "fixture-refresh",
                    "access": {"token": "fixture-access", "expires_at": 4102444800}
                }""".encodeToByteArray(),
            )
            assertFalse(encrypted.readBytes().decodeToString().contains("fixture-refresh"))
            assertFalse(encrypted.readBytes().decodeToString().contains("fixture-access"))
        }
    }

    @Test
    fun restoreAfterProcessRestartAndSignOut() {
        Tracker.openWithSessionStorage(directory.resolve("tracking.sqlite3").absolutePath, vault).use { tracker ->
            assertFalse(tracker.syncStatus().signedIn)
            tracker.restoreSession()
            assertTrue(tracker.syncStatus().signedIn)
            tracker.signOut()
            assertNull(vault.load())
            tracker.restoreSession()
            assertFalse(tracker.syncStatus().signedIn)
        }
    }

    private companion object {
        const val PROJECT = "https://session-fixture.invalid/"
    }
}
