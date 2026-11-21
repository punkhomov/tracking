package dev.punkhomov.tracking

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.punkhomov.tracking.nativecore.SessionKind
import dev.punkhomov.tracking.nativecore.SessionView
import dev.punkhomov.tracking.nativecore.TimerStatus
import dev.punkhomov.tracking.nativecore.TimerView
import dev.punkhomov.tracking.nativecore.Tracker
import dev.punkhomov.tracking.nativecore.TrackerException
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
    val busy: Boolean = false,
    val error: String? = null,
)

class TrackerViewModel(application: Application) : AndroidViewModel(application) {
    private val tracking = application as TrackingApplication
    private val operationLock = Mutex()
    private val state = MutableStateFlow(TrackingState())
    val ui = state.asStateFlow()

    init {
        reload()
    }

    fun reload() = change(readHistory = true) {}
    fun selectKind(kind: SessionKind) = change { it.selectKind(kind) }
    fun start(note: String) = change { it.start(note) }
    fun pause() = change { it.pause() }
    fun resume(note: String?) = change { it.resume(note) }
    fun finish(note: String?) = change(readHistory = true) { it.finish(note) }

    suspend fun refreshTimer() = operationLock.withLock {
        val current = state.value
        if (current.busy || current.timer?.status != TimerStatus.RUNNING) return@withLock
        try {
            val timer = withContext(Dispatchers.IO) { tracking.tracker.timer() }
            state.update { it.copy(timer = timer) }
        } catch (error: TrackerException) {
            state.update { it.copy(error = error.message) }
        }
    }

    private fun change(readHistory: Boolean = false, operation: (Tracker) -> Unit) {
        if (state.value.busy) return
        state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                operationLock.withLock {
                    val previousHistory = state.value.history
                    val updated = withContext(Dispatchers.IO) {
                        val tracker = tracking.tracker
                        operation(tracker)
                        TrackingState(
                            timer = tracker.timer(),
                            history = if (readHistory) tracker.history() else previousHistory,
                        )
                    }
                    state.value = updated
                }
            } catch (error: TrackerException) {
                state.update { it.copy(busy = false, error = error.message) }
            }
        }
    }
}
