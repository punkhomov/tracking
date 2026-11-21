package dev.punkhomov.tracking

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import dev.punkhomov.tracking.nativecore.SessionKind
import dev.punkhomov.tracking.nativecore.SessionView
import dev.punkhomov.tracking.nativecore.TimerStatus
import dev.punkhomov.tracking.nativecore.TimerView
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val model by viewModels<TrackerViewModel>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (true) {
                    model.refreshTimer()
                    delay(1_000)
                }
            }
        }
        setContent {
            val state by model.ui.collectAsStateWithLifecycle()
            MaterialTheme(colorScheme = if (isSystemInDarkTheme()) darkColors else lightColors) {
                TrackingScreen(state, model)
            }
        }
    }
}

private val lightColors = lightColorScheme(
    primary = Color(0xFF1F5C4B),
    surface = Color(0xFFF2F5F1),
    onSurface = Color(0xFF1D2E28),
    surfaceContainerHigh = Color.White,
)

private val darkColors = darkColorScheme(
    primary = Color(0xFF8DD0B6),
    onPrimary = Color(0xFF16221E),
    surface = Color(0xFF16221E),
    onSurface = Color(0xFFE3EDE8),
    surfaceContainerHigh = Color(0xFF202E29),
)

@Composable
private fun TrackingScreen(state: TrackingState, model: TrackerViewModel) {
    var historyOpen by rememberSaveable { mutableStateOf(false) }
    BackHandler(enabled = historyOpen) { historyOpen = false }

    Scaffold { insets ->
        Column(Modifier.fillMaxSize().padding(insets).imePadding()) {
            Text(
                "Tracking",
                Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
                style = MaterialTheme.typography.titleLarge,
            )
            val timer = state.timer
            if (timer == null) {
                Box(Modifier.fillMaxSize().padding(20.dp), contentAlignment = Alignment.Center) {
                    if (state.error == null) {
                        CircularProgressIndicator()
                    } else {
                        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                            ErrorText(state.error, Modifier)
                            Button(onClick = model::reload, enabled = !state.busy) {
                                Text("Повторить")
                            }
                        }
                    }
                }
            } else {
                var note by rememberSaveable(timer.sessionId) { mutableStateOf(timer.note) }
                TabRow(selectedTabIndex = if (historyOpen) 1 else 0) {
                    Tab(!historyOpen, onClick = { historyOpen = false }, text = { Text("Таймер") })
                    Tab(historyOpen, onClick = { historyOpen = true }, text = { Text("История") })
                }
                if (state.error != null) {
                    ErrorText(state.error, Modifier.padding(20.dp))
                }
                if (historyOpen) {
                    HistoryScreen(state.history)
                } else {
                    TimerScreen(timer, note, { note = it }, state.busy, model)
                }
            }
        }
    }
}

@Composable
private fun ErrorText(message: String, modifier: Modifier) {
    Text(
        message,
        modifier.semantics { liveRegion = LiveRegionMode.Polite },
        color = MaterialTheme.colorScheme.error,
    )
}

@Composable
private fun TimerScreen(
    timer: TimerView,
    note: String,
    onNoteChange: (String) -> Unit,
    busy: Boolean,
    model: TrackerViewModel,
) {
    val focus = LocalFocusManager.current
    val ready = timer.status == TimerStatus.READY
    val paused = timer.status == TimerStatus.PAUSED
    val editable = timer.kind == SessionKind.WORK && (ready || paused)
    val editedNote = note.takeIf { paused && editable && it != timer.note }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (ready) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (kind in SessionKind.entries) {
                    FilterChip(
                        selected = timer.kind == kind,
                        onClick = { if (timer.kind != kind) model.selectKind(kind) },
                        label = { Text(kind.label) },
                        enabled = !busy,
                    )
                }
            }
        }
        Card(Modifier.fillMaxWidth()) {
            Column(
                Modifier.padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    when (timer.status) {
                        TimerStatus.READY -> "Готов к запуску"
                        TimerStatus.RUNNING -> "${timer.kind.label} · В процессе"
                        TimerStatus.PAUSED -> "${timer.kind.label} · На паузе"
                    },
                    modifier = Modifier.testTag("timerStatus"),
                    style = MaterialTheme.typography.labelLarge,
                )
                Text(
                    clock(timer.elapsedSeconds),
                    modifier = Modifier.testTag("elapsed"),
                    style = MaterialTheme.typography.displayMedium,
                    fontFamily = FontFamily.Monospace,
                )
                LinearProgressIndicator(
                    progress = {
                        (timer.elapsedSeconds.toFloat() / timer.targetSeconds).coerceIn(0f, 1f)
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
                Text("Цель ${timer.targetSeconds / 60} мин", style = MaterialTheme.typography.bodyMedium)
            }
        }
        if (editable) {
            OutlinedTextField(
                value = note,
                onValueChange = onNoteChange,
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
                label = { Text("Заметка") },
                minLines = 2,
                maxLines = 4,
            )
        } else if (timer.note.isNotEmpty()) {
            Text(timer.note, style = MaterialTheme.typography.bodyLarge)
        }

        if (ready) {
            Button(
                onClick = {
                    focus.clearFocus()
                    model.start(note)
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
            ) {
                Text(if (timer.kind == SessionKind.WORK) "Запустить" else "Начать перерыв")
            }
        } else {
            Button(
                onClick = {
                    focus.clearFocus()
                    model.finish(editedNote)
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
            ) {
                Text("Завершить")
            }
            OutlinedButton(
                onClick = {
                    focus.clearFocus()
                    if (paused) model.resume(editedNote) else model.pause()
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
            ) {
                Text(if (paused) "Продолжить" else "Пауза")
            }
        }
    }
}

@Composable
private fun HistoryScreen(sessions: List<SessionView>) {
    if (sessions.isEmpty()) {
        Text("Завершённых сессий пока нет.", Modifier.padding(20.dp))
    } else {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(vertical = 8.dp)) {
            items(sessions, key = { it.id }) { session ->
                val date = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                    .format(Date(session.startMillis))
                ListItem(
                    headlineContent = { Text(session.note.ifBlank { session.kind.label }) },
                    supportingContent = { Text("${session.kind.label} · $date") },
                    trailingContent = { Text(clock(session.durationMillis / 1_000)) },
                )
            }
        }
    }
}

private val SessionKind.label: String
    get() = when (this) {
        SessionKind.WORK -> "Работа"
        SessionKind.BREAK -> "Перерыв"
    }

private fun clock(seconds: Long): String =
    if (seconds >= 3_600) {
        String.format(Locale.ROOT, "%d:%02d:%02d", seconds / 3_600, seconds / 60 % 60, seconds % 60)
    } else {
        String.format(Locale.ROOT, "%02d:%02d", seconds / 60, seconds % 60)
    }
