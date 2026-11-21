package dev.punkhomov.tracking

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.punkhomov.tracking.nativecore.ReportSpan
import dev.punkhomov.tracking.nativecore.ReportView
import dev.punkhomov.tracking.nativecore.DayActivity
import dev.punkhomov.tracking.nativecore.SessionKind
import dev.punkhomov.tracking.nativecore.SettingsView
import dev.punkhomov.tracking.nativecore.TimelineEntry
import dev.punkhomov.tracking.nativecore.TimelineView
import dev.punkhomov.tracking.nativecore.TrackerException
import dev.punkhomov.tracking.nativecore.ZoneAppearance
import java.time.LocalDate
import java.time.ZoneId
import kotlin.math.roundToInt

@Composable
internal fun TimelineScreen(state: TrackingState, model: TrackerViewModel) {
    var day by rememberSaveable { mutableStateOf(LocalDate.now().toString()) }
    var fullDay by rememberSaveable { mutableStateOf(true) }
    var timeline by remember { mutableStateOf<TimelineView?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val date = LocalDate.parse(day)
    val timeZone = ZoneId.systemDefault()
    LaunchedEffect(day, state.revision, state.timer?.sessionId, state.timer?.status, state.timer?.elapsedSeconds?.div(60)) {
        try {
            timeline = model.read { it.timeline(day, timeZone.id) }
            error = null
        } catch (failure: TrackerException) {
            error = failure.message
        }
    }
    val start = date.atStartOfDay(timeZone).toInstant().toEpochMilli()
    val end = date.plusDays(1).atStartOfDay(timeZone).toInstant().toEpochMilli()
    val rows = timeline?.entries
    val visible = if (fullDay || rows?.any { !it.planned } != true) emptyList() else rows.filter { !it.planned || it.fullRange }
    val rangeStart = visible.minOfOrNull { it.startMillis }?.let { (it - 900_000).coerceAtLeast(start) } ?: start
    val rangeEnd = visible.maxOfOrNull { it.endMillis }?.let { (it + 900_000).coerceAtMost(end) } ?: end
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            DateNavigation(date, state.settings, { day = it.toString() })
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(
                    selected = fullDay,
                    onClick = { fullDay = true },
                    shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                ) { Text("Весь день") }
                SegmentedButton(
                    selected = !fullDay,
                    onClick = { fullDay = false },
                    shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                ) { Text("От первой сессии") }
            }
        }
        error?.let { item { ErrorText(it) } }
        when {
            rows == null && error == null -> item { CircularProgressIndicator() }
            rows?.isEmpty() == true -> item { Text("На этот день нет сессий и зон.") }
            rows != null -> items(rows.filter { it.endMillis > rangeStart && it.startMillis < rangeEnd }, key = { it.id }) { entry ->
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    val prefix = when {
                        entry.nested -> "Вложенная зона"
                        entry.planned -> "Зона"
                        else -> "Сессия"
                    }
                    val formatter = hourFormat(state.settings)
                    val from = localDate(entry.startMillis).format(formatter)
                    val to = localDate(entry.endMillis).format(formatter)
                    Text(entry.name, style = MaterialTheme.typography.titleMedium)
                    Text("$prefix · $from–$to", style = MaterialTheme.typography.bodySmall)
                    IntervalBar(entry, rangeStart, rangeEnd)
                }
            }
        }
        timeline?.breakdown?.let { groups ->
            if (groups.isNotEmpty()) {
                item { Text("Сводка по зонам", style = MaterialTheme.typography.titleLarge) }
                items(groups, key = { "group-${it.id}" }) { group ->
                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(group.name, style = MaterialTheme.typography.titleMedium)
                        if (group.plannedMillis > 0) Text("План · ${clock(group.plannedMillis / 1_000)}")
                        group.categories.forEach { category ->
                            Text("${category.name} · ${clock(category.activeMillis / 1_000)} · ${category.sessions} сессий")
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun IntervalBar(entry: TimelineEntry, start: Long, end: Long) {
    val duration = (end - start).coerceAtLeast(1).toDouble()
    val left = ((entry.startMillis - start) / duration).coerceIn(0.0, 1.0).toFloat()
    val right = ((entry.endMillis - start) / duration).coerceIn(left.toDouble(), 1.0).toFloat()
    val shape = RoundedCornerShape(4.dp)
    val accent = colorOrNull(entry.color) ?: MaterialTheme.colorScheme.primary
    BoxWithConstraints(
        Modifier.fillMaxWidth().height(8.dp).background(MaterialTheme.colorScheme.surfaceVariant, shape),
    ) {
        val marker = Modifier.offset(x = maxWidth * left).width((maxWidth * (right - left)).coerceAtLeast(2.dp)).height(8.dp)
        val style = when (entry.style) {
            ZoneAppearance.SOLID -> marker.background(accent.copy(alpha = if (entry.planned) 0.48f else 1f), shape)
            ZoneAppearance.BORDER -> marker.border(1.dp, accent, shape)
            ZoneAppearance.HATCHED -> marker.clip(shape).drawBehind {
                val step = 5.dp.toPx()
                var x = -size.height
                while (x < size.width) {
                    drawLine(accent, Offset(x, size.height), Offset(x + size.height, 0f), strokeWidth = 1.dp.toPx())
                    x += step
                }
            }
        }
        Box(style)
    }
}

@Composable
internal fun StatisticsScreen(state: TrackingState, model: TrackerViewModel) {
    var day by rememberSaveable { mutableStateOf(LocalDate.now().toString()) }
    var span by rememberSaveable { mutableStateOf(ReportSpan.WEEK) }
    var kind by rememberSaveable { mutableStateOf<SessionKind?>(null) }
    var report by remember { mutableStateOf<ReportView?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val timeZone = ZoneId.systemDefault().id
    val date = LocalDate.parse(day)
    LaunchedEffect(day, span, kind, state.revision, timeZone) {
        try {
            report = model.read { it.report(day, span, kind, timeZone) }
            error = null
        } catch (failure: TrackerException) {
            error = failure.message
        }
    }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            DateNavigation(date, state.settings, { day = it.toString() }, span)
            Picker("Период", span, listOf(ReportSpan.DAY to "День", ReportSpan.WEEK to "Неделя", ReportSpan.MONTH to "Месяц")) {
                span = it
            }
            Picker("Сессии", kind, listOf(null to "Все") + SessionKind.entries.map { it to it.label }) { kind = it }
        }
        error?.let { item { ErrorText(it) } }
        val result = report
        if (result == null && error == null) {
            item { CircularProgressIndicator() }
        }
        if (result != null) {
            item {
                Text("${result.sessions} сессий · ${clock(result.activeMillis / 1_000)} активности")
                result.alignment?.let {
                    Text("Соответствие зонам · ${(it * 100).roundToInt()}%")
                }
                if (result.uncategorizedMillis > 0) {
                    Text("В зонах без категории · ${clock(result.uncategorizedMillis / 1_000)}")
                }
            }
            val max = result.days.maxOfOrNull { it.workMillis }?.coerceAtLeast(1) ?: 1
            if (span != ReportSpan.DAY) {
                item { ActivityCalendar(result.days, max, state.settings, span == ReportSpan.MONTH) }
            }
            items(result.days.withIndex().toList(), key = { it.value.date }) { (index, row) ->
                Text("${LocalDate.parse(row.date).format(dateFormat(state.settings))} · ${clock(row.workMillis / 1_000)} работы")
                if (span != ReportSpan.DAY) Text("${result.counts.getOrElse(index) { 0 }} сессий")
                LinearProgressIndicator(
                    progress = { row.workMillis.toFloat() / max },
                    modifier = Modifier.fillMaxWidth(),
                )
                row.alignment?.let {
                    Text("По плану · ${(it * 100).roundToInt()}%", style = MaterialTheme.typography.bodySmall)
                }
            }
            if (span == ReportSpan.DAY) {
                val hours = result.counts.withIndex().filter { it.value > 0 }
                if (hours.isNotEmpty()) item { Text("Начатые сессии по часам", style = MaterialTheme.typography.titleMedium) }
                items(hours, key = { it.index }) { hour ->
                    Text("${hour.index.toString().padStart(2, '0')}:00 · ${hour.value}")
                }
            }
        }
    }
}

@Composable
private fun ActivityCalendar(days: List<DayActivity>, max: Long, settings: SettingsView?, month: Boolean) {
    val first = when (settings?.firstDayOfWeek) {
        "sunday" -> 7
        "saturday" -> 6
        else -> 1
    }
    val offset = if (month && days.isNotEmpty()) (LocalDate.parse(days.first().date).dayOfWeek.value - first + 7) % 7 else 0
    val rows = (offset + days.size + 6) / 7
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text("Активность по дням", style = MaterialTheme.typography.titleMedium)
        repeat(rows) { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                repeat(7) { column ->
                    val day = days.getOrNull(row * 7 + column - offset)
                    val position = Modifier.weight(1f).height(36.dp)
                    if (day == null) {
                        Box(position)
                    } else {
                        val date = LocalDate.parse(day.date)
                        val color = if (day.workMillis == 0L) MaterialTheme.colorScheme.surfaceVariant
                        else MaterialTheme.colorScheme.primary.copy(alpha = 0.12f + 0.34f * day.workMillis / max)
                        val description = "${date.format(dateFormat(settings))} · ${clock(day.workMillis / 1_000)} работы"
                        Box(
                            position.background(color, RoundedCornerShape(6.dp)).semantics { contentDescription = description },
                            contentAlignment = Alignment.Center,
                        ) { Text(date.dayOfMonth.toString()) }
                    }
                }
            }
        }
    }
}

@Composable
private fun DateNavigation(
    day: LocalDate,
    settings: SettingsView?,
    update: (LocalDate) -> Unit,
    span: ReportSpan = ReportSpan.DAY,
) {
    fun move(amount: Long) = when (span) {
        ReportSpan.DAY -> day.plusDays(amount)
        ReportSpan.WEEK -> day.plusWeeks(amount)
        ReportSpan.MONTH -> day.plusMonths(amount)
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        TextButton(onClick = { update(move(-1)) }) { Text("Назад") }
        Text(day.format(dateFormat(settings)), style = MaterialTheme.typography.titleMedium)
        TextButton(onClick = { update(move(1)) }) { Text("Вперёд") }
    }
    if (day != LocalDate.now()) {
        TextButton(onClick = { update(LocalDate.now()) }) { Text("Сегодня") }
    }
}
