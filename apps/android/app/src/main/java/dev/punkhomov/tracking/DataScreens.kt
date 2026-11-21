package dev.punkhomov.tracking

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import dev.punkhomov.tracking.nativecore.CategoryView
import dev.punkhomov.tracking.nativecore.SessionDetails
import dev.punkhomov.tracking.nativecore.SessionDraft
import dev.punkhomov.tracking.nativecore.SessionKind
import dev.punkhomov.tracking.nativecore.SessionView
import dev.punkhomov.tracking.nativecore.SettingsView
import dev.punkhomov.tracking.nativecore.ZoneAppearance
import dev.punkhomov.tracking.nativecore.ZoneDraft
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter

@Composable
internal fun HistoryScreen(state: TrackingState, model: TrackerViewModel) {
    var query by rememberSaveable { mutableStateOf("") }
    var kind by rememberSaveable { mutableStateOf<SessionKind?>(null) }
    var category by rememberSaveable { mutableStateOf<String?>(null) }
    var from by rememberSaveable { mutableStateOf("") }
    var through by rememberSaveable { mutableStateOf("") }
    var showFilters by rememberSaveable { mutableStateOf(false) }
    var edit by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<SessionView?>(null) }
    var delete by remember { mutableStateOf<SessionView?>(null) }
    var clear by remember { mutableStateOf(false) }
    val names = remember(state.categories) { state.categories.associateBy { it.id } }
    val rows = remember(state.history, names, query, kind, category, from, through) {
        val first = parseDate(from)
        val last = parseDate(through)
        state.history.filter { session ->
            val date = localDate(session.startMillis).toLocalDate()
            (kind == null || session.kind == kind) &&
                (category == null || session.categoryId == category) &&
                (first == null || date >= first) && (last == null || date <= last) &&
                (session.note.contains(query.trim(), ignoreCase = true) ||
                    names[session.categoryId]?.name?.contains(query.trim(), ignoreCase = true) == true)
        }
    }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        item {
            Field("Поиск по заметке или категории", query) { query = it }
            Row {
                TextButton(onClick = { showFilters = !showFilters }) { Text("Фильтры") }
                TextButton(onClick = { selected = null; edit = true }) { Text("Добавить сессию") }
            }
            if (showFilters) {
                Picker("Тип", kind, listOf(null to "Все") + SessionKind.entries.map { it to it.label }) { kind = it }
                Picker("Категория", category, listOf(null to "Все") + state.categories.map { it.id to it.name }) { category = it }
                Field("С даты (ГГГГ-ММ-ДД)", from) { from = it }
                Field("По дату (ГГГГ-ММ-ДД)", through) { through = it }
                if ((from.isNotBlank() && parseDate(from) == null) || (through.isNotBlank() && parseDate(through) == null)) {
                    ErrorText("Проверьте даты фильтра.")
                }
            }
        }
        if (rows.isEmpty()) item { Text("Сессий по этим условиям нет.", Modifier.padding(8.dp)) }
        items(rows, key = { it.id }) { session ->
            val savedCategory = names[session.categoryId]
            ListItem(
                headlineContent = { Text(session.note.ifBlank { savedCategory?.name ?: session.kind.label }) },
                supportingContent = {
                    val date = localDate(session.startMillis)
                    Text(
                        "${date.toLocalDate().format(dateFormat(state.settings))} · " +
                            "${date.format(hourFormat(state.settings))} · ${session.kind.label}",
                    )
                },
                trailingContent = { Text(clock(session.durationMillis / 1_000)) },
            )
            Row(Modifier.horizontalScroll(rememberScrollState())) {
                TextButton(onClick = { selected = session; edit = true }) { Text("Изменить") }
                if (state.timer?.status == dev.punkhomov.tracking.nativecore.TimerStatus.READY) {
                    TextButton(onClick = { model.change { it.reopen(session.id) } }) { Text("Вернуть") }
                }
                TextButton(onClick = { delete = session }) { Text("Удалить") }
            }
        }
        if (state.history.isNotEmpty()) item {
            TextButton(onClick = { clear = true }) { Text("Очистить историю") }
        }
    }
    if (edit) key(selected?.id) {
        SessionEditor(selected, state, model, onDismiss = { edit = false })
    }
    delete?.let { session ->
        ConfirmAction("Удалить сессию?", "Удалить", onDismiss = { delete = null }) {
            model.change(onSuccess = { delete = null }) { it.deleteSession(session.id) }
        }
    }
    if (clear) {
        ConfirmAction("Очистить историю сессий?", "Очистить", onDismiss = { clear = false }) {
            model.change(onSuccess = { clear = false }) { it.clearHistory() }
        }
    }
}

@Composable
private fun SessionEditor(
    original: SessionView?,
    state: TrackingState,
    model: TrackerViewModel,
    onDismiss: () -> Unit,
) {
    val defaultEnd = remember { LocalDateTime.now().withNano(0) }
    val startLabel = remember(original) {
        (original?.let { localDate(it.startMillis) } ?: defaultEnd.minusMinutes(25)).format(EDIT_DATE)
    }
    val endLabel = remember(original) {
        (original?.let { localDate(it.endMillis) } ?: defaultEnd).format(EDIT_DATE)
    }
    var start by rememberSaveable { mutableStateOf(startLabel) }
    var end by rememberSaveable { mutableStateOf(endLabel) }
    var kind by rememberSaveable { mutableStateOf(original?.kind ?: SessionKind.WORK) }
    var note by rememberSaveable { mutableStateOf(original?.note.orEmpty()) }
    var category by rememberSaveable { mutableStateOf(original?.categoryId) }
    var error by remember { mutableStateOf<String?>(null) }
    EditorDialog(
        if (original == null) "Добавить сессию" else "Изменить сессию",
        state.busy,
        onDismiss,
        onSave = {
            val first = if (original != null && start == startLabel) original.startMillis else parseLocalTime(start)
            val last = if (original != null && end == endLabel) original.endMillis else parseLocalTime(end)
            if (first == null || last == null || last <= first) {
                error = "Проверьте даты и время. Конец должен быть позже начала."
            } else {
                model.change(onSuccess = onDismiss) {
                    it.saveSession(SessionDraft(original?.id, kind, first, last, SessionDetails(note, category)))
                }
            }
        },
    ) {
        Picker("Тип", kind, SessionKind.entries.map { it to it.label }) { kind = it }
        CategoryPicker(state.categories, category, { category = it })
        Field("Заметка", note) { note = it }
        Field("Начало (ГГГГ-ММ-ДД ЧЧ:ММ)", start) { start = it }
        Field("Конец (ГГГГ-ММ-ДД ЧЧ:ММ)", end) { end = it }
        (error ?: state.error)?.let { ErrorText(it) }
    }
}

@Composable
internal fun CategoriesScreen(state: TrackingState, model: TrackerViewModel) {
    var editing by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<CategoryView?>(null) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(12.dp)) {
        item {
            Button(onClick = { selected = null; editing = true }) { Text("Добавить категорию") }
        }
        items(state.categories, key = { it.id }) { category ->
            ListItem(
                headlineContent = { Text(category.name) },
                supportingContent = { if (category.archived) Text("В архиве") },
                leadingContent = { Swatch(category.color) },
            )
            Row {
                TextButton(onClick = { selected = category; editing = true }) { Text("Изменить") }
                TextButton(enabled = !state.busy, onClick = {
                    model.change { it.archiveCategory(category.id, !category.archived) }
                }) { Text(if (category.archived) "Восстановить" else "В архив") }
            }
        }
    }
    if (editing) key(selected?.id) {
        var name by rememberSaveable { mutableStateOf(selected?.name.orEmpty()) }
        var color by rememberSaveable { mutableStateOf(selected?.color ?: "#1F5C4B") }
        var error by remember { mutableStateOf<String?>(null) }
        EditorDialog("Категория", state.busy, { editing = false }, onSave = {
            if (name.isBlank() || !validColor(color)) {
                error = "Введите название и цвет в формате #RRGGBB."
            } else {
                model.change(onSuccess = { editing = false }) { it.saveCategory(selected?.id, name, color) }
            }
        }) {
            Field("Название", name) { name = it }
            ColorInput(color) { color = it }
            (error ?: state.error)?.let { ErrorText(it) }
        }
    }
}

@Composable
internal fun ZonesScreen(state: TrackingState, model: TrackerViewModel) {
    var editing by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<ZoneDraft?>(null) }
    var delete by remember { mutableStateOf<ZoneDraft?>(null) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(12.dp)) {
        item {
            Button(onClick = { selected = null; editing = true }) { Text("Добавить зону") }
        }
        items(state.zones, key = { it.id.orEmpty() }) { zone ->
            ListItem(
                headlineContent = { Text(zone.name) },
                supportingContent = {
                    Text(buildString {
                        append("${zone.start}–${zone.end}")
                        val parent = state.zones.firstOrNull { it.id == zone.parent }
                        if (parent != null) append(" · ${parent.name}")
                        zone.endsOn?.let { append(" · до $it") }
                    })
                },
                leadingContent = { zone.color?.let { Swatch(it) } },
            )
            Row {
                TextButton(onClick = { selected = zone; editing = true }) { Text("Изменить") }
                TextButton(onClick = { delete = zone }) { Text("Удалить") }
            }
        }
    }
    if (editing) key(selected?.id) {
        ZoneEditor(selected, state, model) { editing = false }
    }
    delete?.let { zone ->
        ConfirmAction(
            if (state.zones.any { it.parent == zone.id }) "Удалить зону и её вложенные зоны?" else "Удалить зону?",
            "Удалить",
            onDismiss = { delete = null },
        ) {
            model.change(onSuccess = { delete = null }) { it.deleteZone(checkNotNull(zone.id)) }
        }
    }
}

@Composable
private fun ZoneEditor(original: ZoneDraft?, state: TrackingState, model: TrackerViewModel, onDismiss: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(original?.name.orEmpty()) }
    var start by rememberSaveable { mutableStateOf(original?.start ?: "09:00") }
    var end by rememberSaveable { mutableStateOf(original?.end ?: "18:00") }
    var startDate by rememberSaveable { mutableStateOf(original?.startsOn ?: if (original == null) LocalDate.now().toString() else "") }
    var endDate by rememberSaveable { mutableStateOf(original?.endsOn.orEmpty()) }
    var exceptions by rememberSaveable { mutableStateOf(original?.excludedDates?.joinToString(", ").orEmpty()) }
    var mask by rememberSaveable { mutableStateOf(original?.weekdays?.fold(0) { sum, day -> sum or (1 shl day.toInt()) } ?: 62) }
    var parent by rememberSaveable { mutableStateOf(original?.parent) }
    var color by rememberSaveable { mutableStateOf(original?.color.orEmpty()) }
    var style by rememberSaveable { mutableStateOf(original?.style ?: ZoneAppearance.SOLID) }
    var fullRange by rememberSaveable { mutableStateOf(original?.fullRange ?: false) }
    var excluded by rememberSaveable { mutableStateOf(original?.excludeFromBreakdown ?: false) }
    var category by rememberSaveable { mutableStateOf(original?.categoryId) }
    var error by remember { mutableStateOf<String?>(null) }
    EditorDialog("Зона", state.busy, onDismiss, onSave = {
        if (color.isNotBlank() && !validColor(color)) {
            error = "Введите цвет в формате #RRGGBB."
        } else {
            val draft = ZoneDraft(
                original?.id, name, start, end,
                (0..6).filter { mask and (1 shl it) != 0 }.map { it.toByte() }.toByteArray(),
                startDate.trim().takeIf { it.isNotEmpty() }, endDate.trim().takeIf { it.isNotEmpty() },
                exceptions.split(',', '\n').map(String::trim).filter(String::isNotEmpty),
                parent, color.trim().takeIf { it.isNotEmpty() }, style, fullRange, excluded, category,
            )
            model.change(onSuccess = onDismiss) { it.saveZone(draft) }
        }
    }) {
        Field("Название", name) { name = it }
        Field("Начало (ЧЧ:ММ)", start) { start = it }
        Field("Конец (ЧЧ:ММ)", end) { end = it }
        Text("Дни недели")
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            val days = listOf("Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб")
            for (day in listOf(1, 2, 3, 4, 5, 6, 0)) {
                val bit = 1 shl day
                FilterChip(selected = mask and bit != 0, onClick = { mask = mask xor bit }, label = { Text(days[day]) })
            }
        }
        Field("Действует с (ГГГГ-ММ-ДД)", startDate) { startDate = it }
        Field("Действует по (ГГГГ-ММ-ДД)", endDate) { endDate = it }
        Field("Исключения через запятую", exceptions, singleLine = false) { exceptions = it }
        val roots = state.zones.filter { it.id != original?.id && it.parent == null }
        Picker("Основная зона", parent, listOf(null to "Без вложения") + roots.map { it.id to it.name }) { parent = it }
        if (parent == null) {
            CategoryPicker(state.categories, category, { category = it })
            Picker(
                "Отображение", style,
                listOf(ZoneAppearance.SOLID to "Заливка", ZoneAppearance.BORDER to "Контур", ZoneAppearance.HATCHED to "Штриховка"),
            ) { style = it }
            ColorInput(color) { color = it }
            Toggle("Исключить из разбивки", excluded) { excluded = it }
        }
        Toggle("Показывать весь диапазон", fullRange) { fullRange = it }
        (error ?: state.error)?.let { ErrorText(it) }
    }
}

@Composable
internal fun Field(label: String, value: String, singleLine: Boolean = true, onChange: (String) -> Unit) {
    OutlinedTextField(
        value, onChange, Modifier.fillMaxWidth(),
        label = { Text(label) }, singleLine = singleLine,
    )
}

@Composable
internal fun <T> Picker(label: String, value: T, options: List<Pair<T, String>>, onChange: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val name = options.firstOrNull { it.first == value }?.second ?: "Выбрать"
    Box {
        OutlinedButton(onClick = { open = true }) { Text("$label · $name") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (option, title) ->
                DropdownMenuItem(text = { Text(title) }, onClick = { open = false; onChange(option) })
            }
        }
    }
}

@Composable
internal fun CategoryPicker(categories: List<CategoryView>, category: String?, onChange: (String?) -> Unit) {
    Picker(
        "Категория", category,
        listOf(null to "Без категории") + categories.filter { !it.archived || it.id == category }.map { it.id to it.name },
        onChange,
    )
}

@Composable
internal fun Toggle(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Checkbox(checked = checked, onCheckedChange = onChange)
        Text(label)
    }
}

@Composable
internal fun EditorDialog(
    title: String, busy: Boolean, onDismiss: () -> Unit, onSave: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column(
                Modifier.heightIn(max = 440.dp).imePadding().verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                content = content,
            )
        },
        confirmButton = { TextButton(onClick = onSave, enabled = !busy) { Text("Сохранить") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

@Composable
internal fun ConfirmAction(title: String, action: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss, title = { Text(title) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(action) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

private val palette = listOf("#1F5C4B", "#3C67A6", "#965F26", "#8B4A83", "#B13E47")

@Composable
private fun ColorInput(value: String, onChange: (String) -> Unit) {
    Picker("Цвет", value, palette.map { it to it }, onChange)
    Field("Цвет (#RRGGBB)", value, onChange = onChange)
}

@Composable
private fun Swatch(value: String) {
    Box(Modifier.size(16.dp).background(colorOrNull(value) ?: MaterialTheme.colorScheme.primary, CircleShape))
}

internal fun colorOrNull(value: String?): Color? =
    value?.let { runCatching { Color(android.graphics.Color.parseColor(it)) }.getOrNull() }

private fun validColor(value: String) = value.matches(Regex("#[0-9a-fA-F]{6}"))

internal fun localDate(millis: Long) = Instant.ofEpochMilli(millis).atZone(ZoneId.systemDefault()).toLocalDateTime()

internal fun dateFormat(settings: SettingsView?) = DateTimeFormatter.ofPattern(
    when (settings?.dateFormat) {
        "YYYY-MM-DD" -> "yyyy-MM-dd"
        "MM/DD/YYYY" -> "MM/dd/yyyy"
        else -> "dd.MM.yyyy"
    },
)

internal fun hourFormat(settings: SettingsView?) = DateTimeFormatter.ofPattern(
    if (settings?.hourFormat == "12h") "h:mm a" else "HH:mm",
)

private val EDIT_DATE = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss")

private fun parseDate(text: String): LocalDate? =
    runCatching { LocalDate.parse(text.trim()) }.getOrNull()

private fun parseLocalTime(text: String): Long? =
    runCatching {
        LocalDateTime.parse(text.trim().replace(' ', 'T')).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
    }.getOrNull()
