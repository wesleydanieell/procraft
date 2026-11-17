package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.geometry.Offset
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AppPressureCalibrationTest {
    @Test fun presetsKeepFullRangeAndRiseMonotonically() {
        assertEquals(listOf("Light touch", "Balanced", "Firm"), pressurePresets.map { it.label })
        pressurePresets.forEach { preset ->
            assertEquals(Offset(0f, 0f), preset.points.first())
            assertEquals(Offset(1f, 1f), preset.points.last())
            assertTrue(preset.points.zipWithNext().all { (a, b) -> b.x > a.x && b.y >= a.y })
        }
    }

    @Test fun lightAndFirmPresetsBracketBalancedPressure() {
        val light = pressurePresets.first { it.label == "Light touch" }
        val firm = pressurePresets.first { it.label == "Firm" }
        assertTrue(light.points.first { it.x == 0.35f }.y > 0.35f)
        assertTrue(firm.points.first { it.x == 0.35f }.y < 0.35f)
    }

    @Test fun sampleLookupInterpolatesAndClampsLikeNativeInput() {
        val samples = listOf(0f, 0.8f, 1f)
        assertEquals(0.4f, calibratedPressure(0.25f, samples), 0.00001f)
        assertEquals(0.9f, calibratedPressure(0.75f, samples), 0.00001f)
        assertEquals(0f, calibratedPressure(-1f, samples), 0.00001f)
        assertEquals(1f, calibratedPressure(2f, samples), 0.00001f)
        assertEquals(0f, calibratedPressure(Float.NaN, samples), 0.00001f)
        assertEquals(0f, calibratedPressure(0.5f, emptyList()), 0.00001f)
    }
}
