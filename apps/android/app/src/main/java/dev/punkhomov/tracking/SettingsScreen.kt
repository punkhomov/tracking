package dev.punkhomov.tracking

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.punkhomov.tracking.nativecore.ArchivePart
import dev.punkhomov.tracking.nativecore.SettingsView
import dev.punkhomov.tracking.nativecore.SyncChoice

@Composable
internal fun SettingsScreen(state: TrackingState, cloud: CloudScreenState, model: TrackerViewModel) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        CloudSettings(cloud, model)
        HorizontalDivider()
        state.settings?.let { TimerSettings(it, state.busy, model) }
        HorizontalDivider()
        ArchiveSettings(model)
    }
}

@Composable
private fun CloudSettings(cloud: CloudScreenState, model: TrackerViewModel) {
    val status = cloud.status
    var editing by rememberSaveable(status?.url) { mutableStateOf(status?.url == null) }
    var url by rememberSaveable(status?.url) { mutableStateOf(status?.url.orEmpty()) }
    var key by rememberSaveable(status?.key) { mutableStateOf(status?.key.orEmpty()) }
    var email by rememberSaveable { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    Text("Синхронизация Supabase", style = MaterialTheme.typography.titleLarge)
    cloud.error?.let { ErrorText(it) }
    if (status == null) return
    if (editing) {
        Field("URL проекта", url) { url = it }
        Field("Публичный ключ", key) { key = it }
        Button(enabled = !cloud.busy, onClick = {
            model.cloudAction(codeSent = false, onSuccess = { editing = false }) { it.configureSync(url, key) }
        }) { Text("Сохранить подключение") }
        if (status.url != null) {
            TextButton(onClick = { editing = false }) { Text("Отмена") }
        }
        return
    }
    status.url?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
    if (status.signedIn) {
        when {
            status.timerConflict -> ErrorText("Общий таймер изменился на другом устройстве. Локальные действия сохранены. Синхронизация приостановлена.")
            status.timerPending -> Text("Таймер ожидает синхронизации.")
            status.datasetConflicts != null -> {
                Text("Конфликтующих записей: ${status.datasetConflicts}")
                OutlinedButton(enabled = !cloud.busy, onClick = {
                    model.cloudAction { it.synchronize(SyncChoice.LOCAL) }
                }) { Text("Локальные изменения") }
                OutlinedButton(enabled = !cloud.busy, onClick = {
                    model.cloudAction { it.synchronize(SyncChoice.REMOTE) }
                }) { Text("Облачные изменения") }
            }
            else -> {
                if (!status.needsConfirmation) Text("Автосинхронизация включена.")
                Button(enabled = !cloud.busy, onClick = {
                    model.cloudAction { it.synchronize(SyncChoice.MERGE) }
                }) {
                    Text(if (status.needsConfirmation) "Объединить с этим аккаунтом" else "Синхронизировать")
                }
            }
        }
        TextButton(enabled = !cloud.busy, onClick = {
            model.cloudAction(codeSent = false) { it.signOut() }
        }) { Text("Выйти на этом устройстве") }
    } else {
        Field("Email", email) { email = it }
        if (cloud.codeSent) {
            Field("Код из письма", code) { code = it }
            Button(enabled = !cloud.busy, onClick = {
                model.cloudAction(onSuccess = { code = "" }) { it.verifyCode(email, code) }
            }) { Text("Войти") }
        }
        OutlinedButton(enabled = !cloud.busy, onClick = {
            model.cloudAction(codeSent = true) { it.requestCode(email) }
        }) { Text(if (cloud.codeSent) "Отправить код ещё раз" else "Отправить код") }
        TextButton(enabled = !cloud.busy, onClick = { editing = true }) { Text("Изменить подключение") }
    }
}

@Composable
private fun TimerSettings(settings: SettingsView, busy: Boolean, model: TrackerViewModel) {
    var work by rememberSaveable(settings.workMinutes) { mutableStateOf(settings.workMinutes.toString()) }
    var rest by rememberSaveable(settings.breakMinutes) { mutableStateOf(settings.breakMinutes.toString()) }
    var reminder by rememberSaveable(settings.reminderIntervalMinutes) {
        mutableStateOf(settings.reminderIntervalMinutes.toString())
    }
    var pause by rememberSaveable(settings.minPauseSeconds) { mutableStateOf(settings.minPauseSeconds.toString()) }
    var autoBreak by rememberSaveable(settings.autoStartBreak) { mutableStateOf(settings.autoStartBreak) }
    var autoWork by rememberSaveable(settings.autoStartPomodoro) { mutableStateOf(settings.autoStartPomodoro) }
    var theme by rememberSaveable(settings.theme) { mutableStateOf(settings.theme) }
    var firstDay by rememberSaveable(settings.firstDayOfWeek) { mutableStateOf(settings.firstDayOfWeek) }
    var date by rememberSaveable(settings.dateFormat) { mutableStateOf(settings.dateFormat) }
    var hour by rememberSaveable(settings.hourFormat) { mutableStateOf(settings.hourFormat) }
    var invalid by rememberSaveable { mutableStateOf(false) }
    Text("Таймер", style = MaterialTheme.typography.titleLarge)
    Field("Работа, мин", work) { work = it }
    Field("Перерыв, мин", rest) { rest = it }
    Field("Повтор напоминания, мин", reminder) { reminder = it }
    Field("Минимальная пауза, сек", pause) { pause = it }
    Toggle("Автостарт перерыва", autoBreak) { autoBreak = it }
    Toggle("Автостарт после перерыва", autoWork) { autoWork = it }
    Text("Оформление и регион", style = MaterialTheme.typography.titleLarge)
    Picker("Тема", theme, listOf("system" to "Системная", "light" to "Светлая", "dark" to "Тёмная")) { theme = it }
    Picker(
        "Первый день недели", firstDay,
        listOf("monday" to "Понедельник", "sunday" to "Воскресенье", "saturday" to "Суббота"),
    ) { firstDay = it }
    Picker(
        "Формат даты", date,
        listOf("DD.MM.YYYY" to "ДД.ММ.ГГГГ", "YYYY-MM-DD" to "ГГГГ-ММ-ДД", "MM/DD/YYYY" to "ММ/ДД/ГГГГ"),
    ) { date = it }
    Picker("Формат времени", hour, listOf("24h" to "24 часа", "12h" to "12 часов")) { hour = it }
    if (invalid) ErrorText("Введите длительности и интервал числом.")
    Button(enabled = !busy, onClick = {
        val values = listOf(work, rest, reminder, pause).map { it.trim().toIntOrNull() }
        invalid = values.any { it == null }
        if (!invalid) {
            val (workMin, restMin, reminderMin, pauseSeconds) = values.filterNotNull()
            model.change {
                it.saveSettings(SettingsView(
                    workMin, restMin, reminderMin, pauseSeconds,
                    autoBreak, autoWork, firstDay, date, hour, theme,
                ))
            }
        }
    }) { Text("Сохранить настройки") }
    if (Build.VERSION.SDK_INT >= 33) {
        val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
        OutlinedButton(onClick = { permission.launch(Manifest.permission.POST_NOTIFICATIONS) }) {
            Text("Разрешить уведомления")
        }
    }
}

@Composable
private fun ArchiveSettings(model: TrackerViewModel) {
    var part by rememberSaveable { mutableStateOf(ArchivePart.FULL) }
    val import = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) model.previewArchive(uri)
    }
    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) model.exportArchive(uri, part)
    }
    Text("Архив", style = MaterialTheme.typography.titleLarge)
    Picker(
        "Состав", part,
        listOf(ArchivePart.FULL to "Всё", ArchivePart.HISTORY to "История", ArchivePart.SETTINGS to "Настройки"),
    ) { part = it }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton(onClick = { import.launch(arrayOf("application/json", "text/plain")) }) { Text("Импорт JSON") }
        OutlinedButton(onClick = { export.launch("tracking.json") }) { Text("Экспорт JSON") }
    }
    val preview by model.pendingImport.collectAsStateWithLifecycle()
    preview?.let {
        var replace by rememberSaveable { mutableStateOf(false) }
        AlertDialog(
            onDismissRequest = model::dismissImport,
            title = { Text("Импорт архива") },
            text = {
                Column {
                    Text("Сессии: ${it.info.sessions} · Категории: ${it.info.categories} · Зоны: ${it.info.zones}")
                    if (it.info.settings) Text("В архиве есть настройки.")
                    Toggle("Заменить историю", replace) { checked -> replace = checked }
                }
            },
            confirmButton = { TextButton(onClick = { model.importArchive(replace) }) { Text("Импортировать") } },
            dismissButton = { TextButton(onClick = model::dismissImport) { Text("Отмена") } },
        )
    }
}
