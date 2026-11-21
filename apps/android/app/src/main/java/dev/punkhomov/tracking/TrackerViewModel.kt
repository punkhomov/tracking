package dev.punkhomov.tracking

import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.net.Uri
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.punkhomov.tracking.nativecore.ArchiveInfo
import dev.punkhomov.tracking.nativecore.ArchivePart
import dev.punkhomov.tracking.nativecore.CategoryView
import dev.punkhomov.tracking.nativecore.ReminderView
import dev.punkhomov.tracking.nativecore.SessionDetails
import dev.punkhomov.tracking.nativecore.SessionKind
import dev.punkhomov.tracking.nativecore.SessionView
import dev.punkhomov.tracking.nativecore.SettingsView
import dev.punkhomov.tracking.nativecore.SyncView
import dev.punkhomov.tracking.nativecore.TimerStatus
import dev.punkhomov.tracking.nativecore.TimerView
import dev.punkhomov.tracking.nativecore.Tracker
import dev.punkhomov.tracking.nativecore.TrackerException
import dev.punkhomov.tracking.nativecore.ZoneDraft
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

class TrackingApplication : Application() {
    val tracker by lazy { Tracker.open(filesDir.resolve("tracking.sqlite3").absolutePath) }
}

data class TrackingState(
    val timer: TimerView? = null,
    val history: List<SessionView> = emptyList(),
    val categories: List<CategoryView> = emptyList(),
    val zones: List<ZoneDraft> = emptyList(),
    val settings: SettingsView? = null,
    val revision: Long = 0,
    val busy: Boolean = false,
    val error: String? = null,
)

data class CloudScreenState(
    val status: SyncView? = null,
    val busy: Boolean = false,
    val codeSent: Boolean = false,
    val error: String? = null,
)

data class ImportPreview(val json: String, val info: ArchiveInfo)

class TrackerViewModel(application: Application) : AndroidViewModel(application) {
    private val tracking = application as TrackingApplication
    private val operationLock = Mutex()
    private val state = MutableStateFlow(TrackingState())
    val ui = state.asStateFlow()
    private val cloudState = MutableStateFlow(CloudScreenState())
    val cloud = cloudState.asStateFlow()
    private val importState = MutableStateFlow<ImportPreview?>(null)
    val pendingImport = importState.asStateFlow()
    private var lastReminder: ReminderView? = null

    init {
        reload()
        cloudAction {}
    }

    fun reload() = change {}
    fun selectKind(kind: SessionKind) = change(readDataset = false) { it.selectKind(kind) }
    fun start(details: SessionDetails) = change(readDataset = false) { it.start(details) }
    fun pause() = change(readDataset = false) { it.pause() }
    fun resume(details: SessionDetails?) = change(readDataset = false) { it.resume(details) }
    fun finish(details: SessionDetails?) = change { it.finish(details) }
    fun reset() = change(readDataset = false) { it.reset() }

    suspend fun refreshTimer() = operationLock.withLock {
        val current = state.value
        if (current.busy || current.timer?.status != TimerStatus.RUNNING) return@withLock
        try {
            val (timer, reminder) = withContext(Dispatchers.IO) {
                val tracker = tracking.tracker
                tracker.timer() to tracker.reminder()
            }
            state.update { it.copy(timer = timer) }
            if (reminder != null && reminder != lastReminder && notify(reminder)) {
                lastReminder = reminder
            }
        } catch (error: TrackerException) {
            state.update { it.copy(error = error.message) }
        }
    }

    fun change(readDataset: Boolean = true, onSuccess: () -> Unit = {}, operation: (Tracker) -> Unit) {
        if (state.value.busy) return
        state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                operationLock.withLock {
                    val previous = state.value
                    state.value = withContext(Dispatchers.IO) {
                        val tracker = tracking.tracker
                        operation(tracker)
                        if (readDataset) snapshot(tracker, previous.revision + 1)
                        else previous.copy(timer = tracker.timer(), busy = false)
                    }
                }
                onSuccess()
            } catch (error: TrackerException) {
                state.update { it.copy(busy = false, error = error.message) }
            }
        }
    }

    suspend fun refreshSync() {
        if (cloudState.value.status?.signedIn != true || cloudState.value.busy) return
        try {
            val refreshed = withContext(Dispatchers.IO) {
                val tracker = tracking.tracker
                tracker.syncTick()?.let { changed -> changed to tracker.syncStatus() }
            }
            if (refreshed != null) {
                cloudState.update { it.copy(status = refreshed.second, error = null) }
                if (refreshed.first) refreshDataset()
            }
        } catch (error: TrackerException) {
            val status = read { it.syncStatus() }
            cloudState.update { it.copy(status = status, error = error.message) }
        }
    }

    fun cloudAction(codeSent: Boolean? = null, onSuccess: () -> Unit = {}, operation: (Tracker) -> Unit) {
        if (cloudState.value.busy) return
        cloudState.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                val refreshed = withContext(Dispatchers.IO) {
                    val tracker = tracking.tracker
                    operation(tracker)
                    tracker.syncStatus()
                }
                cloudState.update {
                    it.copy(
                        status = refreshed,
                        busy = false,
                        codeSent = if (refreshed.signedIn) false else codeSent ?: it.codeSent,
                    )
                }
                onSuccess()
                refreshDataset()
            } catch (error: TrackerException) {
                cloudState.update { it.copy(busy = false, error = error.message) }
            }
        }
    }

    private suspend fun refreshDataset() = operationLock.withLock {
        val next = read { snapshot(it, state.value.revision + 1) }
        state.update { next.copy(busy = it.busy, error = it.error) }
    }

    private fun snapshot(tracker: Tracker, revision: Long) = TrackingState(
        timer = tracker.timer(),
        history = tracker.history(),
        categories = tracker.categories(),
        zones = tracker.zones(),
        settings = tracker.settings(),
        revision = revision,
    )

    suspend fun <T> read(operation: (Tracker) -> T): T =
        withContext(Dispatchers.IO) { operation(tracking.tracker) }

    fun previewArchive(uri: Uri) {
        archiveAction {
            importState.value = read { tracker ->
                val json = tracking.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
                    ?: error("Не удалось открыть архив.")
                ImportPreview(json, tracker.archiveInfo(json))
            }
        }
    }

    fun importArchive(replace: Boolean) {
        val draft = importState.value ?: return
        change(onSuccess = { importState.value = null }) { it.importArchive(draft.json, replace) }
    }

    fun dismissImport() {
        importState.value = null
    }

    fun exportArchive(uri: Uri, part: ArchivePart) = archiveAction {
        read { tracker ->
            val json = tracker.exportArchive(part)
            val stream = tracking.contentResolver.openOutputStream(uri) ?: error("Не удалось открыть файл.")
            stream.bufferedWriter().use { it.write(json) }
        }
    }

    private fun archiveAction(operation: suspend () -> Unit) {
        viewModelScope.launch {
            try {
                operation()
            } catch (error: TrackerException) {
                state.update { it.copy(error = error.message) }
            } catch (error: java.io.IOException) {
                state.update { it.copy(error = error.message ?: "Ошибка файла.") }
            } catch (error: IllegalStateException) {
                state.update { it.copy(error = error.message) }
            } catch (error: SecurityException) {
                state.update { it.copy(error = error.message ?: "Файл недоступен.") }
            }
        }
    }

    private fun notify(reminder: ReminderView): Boolean {
        if (Build.VERSION.SDK_INT >= 33 &&
            tracking.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return false
        val manager = tracking.getSystemService(NotificationManager::class.java)
        if (!manager.areNotificationsEnabled()) return false
        val channel = NotificationChannel("timer", "Напоминания таймера", NotificationManager.IMPORTANCE_DEFAULT)
        manager.createNotificationChannel(channel)
        val title = if (reminder.kind == SessionKind.WORK) "Время работы истекло" else "Перерыв закончился"
        manager.notify(
            1,
            Notification.Builder(tracking, channel.id)
                .setSmallIcon(R.drawable.tracking_foreground)
                .setContentTitle(title)
                .setAutoCancel(true)
                .build(),
        )
        return true
    }
}
