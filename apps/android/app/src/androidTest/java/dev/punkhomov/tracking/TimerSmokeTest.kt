package dev.punkhomov.tracking

import android.content.res.Configuration
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isEnabled
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.core.view.WindowCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class TimerSmokeTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun startSession() {
        awaitText("Запустить")
        compose.onNode(hasSetTextAction()).performTextInput(NOTE)
        compose.onNodeWithText("Запустить").performScrollTo().performClick()
        awaitText("Работа · В процессе")
        compose.onNodeWithText(NOTE).assertIsDisplayed()
    }

    @Test
    fun restoreAndFinishSession() {
        awaitText("Работа · В процессе")
        compose.onNodeWithText(NOTE).assertIsDisplayed()
        compose.waitUntil(10_000) {
            compose.onAllNodes(hasTestTag("elapsed") and !hasText("00:00"))
                .fetchSemanticsNodes().size == 1
        }
        compose.onNodeWithText("Пауза").performScrollTo().performClick()
        awaitText("Работа · На паузе")
        compose.onNode(hasSetTextAction()).performTextReplacement(EDITED_NOTE)
        compose.onNodeWithText("Завершить").performScrollTo().performClick()
        awaitText("Начать перерыв")
        compose.onNodeWithText("История").performClick()
        compose.onNodeWithText(EDITED_NOTE).assertIsDisplayed()
    }

    @Test
    fun createAndArchiveCategory() {
        awaitText("Категории")
        compose.onNodeWithText("Категории").performScrollTo().performClick()
        compose.onNodeWithText("Добавить категорию").performClick()
        compose.onAllNodes(hasSetTextAction())[0].performTextInput("Тестовая категория")
        compose.onNodeWithText("Сохранить").performClick()
        awaitText("В архив")
        compose.onNodeWithText("Тестовая категория").assertIsDisplayed()
        compose.onNodeWithText("В архив").performScrollTo().performClick()
        awaitText("В архиве")
    }

    @Test
    fun systemBarsFollowSavedTheme() {
        awaitText("Настройки")
        val systemDark = (compose.activity.resources.configuration.uiMode and
            Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        compose.onNodeWithText("Настройки").performScrollTo().performClick()
        awaitText("Тема · Системная")
        awaitSystemBars(lightBackground = !systemDark)

        for ((theme, dark) in listOf("Светлая" to false, "Тёмная" to true, "Системная" to systemDark)) {
            compose.onNode(hasText("Тема · ", substring = true)).performScrollTo().performClick()
            compose.onNodeWithText(theme).performClick()
            compose.onNodeWithText("Сохранить настройки").performScrollTo().performClick()
            compose.waitUntil(10_000) {
                compose.onAllNodes(hasText("Сохранить настройки") and isEnabled())
                    .fetchSemanticsNodes().size == 1
            }
            awaitSystemBars(lightBackground = !dark)
        }
    }

    private fun awaitSystemBars(lightBackground: Boolean) {
        compose.waitUntil(conditionDescription = "System bar icons match the app theme", timeoutMillis = 10_000) {
            val window = compose.activity.window
            compose.runOnIdle {
                val controller = WindowCompat.getInsetsController(window, window.decorView)
                controller.isAppearanceLightStatusBars == lightBackground &&
                    controller.isAppearanceLightNavigationBars == lightBackground
            }
        }
    }

    private fun awaitText(text: String) {
        compose.waitUntil(15_000) {
            compose.onAllNodes(hasText(text)).fetchSemanticsNodes().size == 1
        }
    }

    private companion object {
        const val NOTE = "Заметка до перезапуска"
        const val EDITED_NOTE = "Проверенная сессия"
    }
}
