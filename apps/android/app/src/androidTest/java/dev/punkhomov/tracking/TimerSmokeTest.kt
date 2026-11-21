package dev.punkhomov.tracking

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
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
        awaitText("Тестовая категория")
        compose.onNodeWithText("В архив").performScrollTo().performClick()
        awaitText("В архиве")
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
