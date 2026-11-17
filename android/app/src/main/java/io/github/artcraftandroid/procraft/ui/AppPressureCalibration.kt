package io.github.artcraftandroid.procraft.ui

import android.view.MotionEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInteropFilter
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.curveSamples
import kotlin.math.roundToInt

/** Presets only reshape the global stylus input; they never rewrite the brush's own dynamics. */
internal data class PressurePreset(val label: String, val points: List<Offset>)

internal val pressurePresets = listOf(
    PressurePreset("Light touch", listOf(
        Offset(0f, 0f), Offset(0.12f, 0.30f), Offset(0.35f, 0.62f),
        Offset(0.65f, 0.85f), Offset(1f, 1f),
    )),
    PressurePreset("Balanced", listOf(Offset(0f, 0f), Offset(1f, 1f))),
    PressurePreset("Firm", listOf(
        Offset(0f, 0f), Offset(0.35f, 0.16f), Offset(0.70f, 0.54f), Offset(1f, 1f),
    )),
)

/** Quick calibration and a live pen-input check, alongside the existing editable curve. */
@Composable
internal fun AppPressureCalibration(vm: CanvasViewModel) {
    val selected = pressurePresets.firstOrNull { it.points == vm.inputCurve }?.label
    Text(
        "Choose how much force your pen needs. Light touch makes soft strokes more visible; the curve below is still editable.",
        color = Pc.SlideLabel, fontSize = 12.sp, lineHeight = 17.sp,
        modifier = Modifier.padding(horizontal = 13.dp, vertical = 8.dp),
    )
    Row(Modifier.fillMaxWidth().padding(horizontal = 7.dp)) {
        pressurePresets.forEach { preset ->
            val active = selected == preset.label
            Box(
                Modifier.weight(1f).padding(horizontal = 3.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(if (active) Pc.Accent else Pc.ActionsGroup)
                    .clickable { vm.changeInput(curve = preset.points) }
                    .padding(vertical = 13.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    preset.label, fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                    color = if (active) Color.White else Pc.PanelTitle,
                )
            }
        }
    }
    if (selected == null) {
        Text("Custom curve", color = Pc.SlideLabel, fontSize = 12.sp, modifier = Modifier.padding(start = 13.dp, top = 5.dp))
    }
    Spacer(Modifier.height(12.dp))
    StylusPressureTester(vm.inputCurve)
}

/** Displays the actual Android input and the result of the same curve the Rust engine uses. */
@Composable
private fun StylusPressureTester(curve: List<Offset>) {
    val samples = remember(curve) { curveSamples(curve.flatMap { listOf(it.x, it.y) }, 129u) }
    var raw by remember { mutableFloatStateOf(0f) }
    var detected by remember { mutableStateOf(false) }
    val adjusted = calibratedPressure(raw, samples)

    Column(
        Modifier.fillMaxWidth().padding(horizontal = 7.dp)
            .clip(RoundedCornerShape(14.dp)).background(Pc.ActionsGroup)
            .pointerInteropFilter { event ->
                // Don't confuse finger touch with the stylus's pressure (finger input is fixed in the engine).
                val penIndex = (0 until event.pointerCount).firstOrNull {
                    event.getToolType(it) == MotionEvent.TOOL_TYPE_STYLUS ||
                        event.getToolType(it) == MotionEvent.TOOL_TYPE_ERASER
                } ?: return@pointerInteropFilter false
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE -> {
                        val value = event.getPressure(penIndex)
                        if (value.isFinite()) {
                            raw = value.coerceAtLeast(0f)
                            detected = true
                        }
                    }
                }
                true
            }
            .padding(15.dp),
    ) {
        Text("Pen pressure test", color = Pc.PanelTitle, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
        Text(
            if (detected) "Last reading — press here again to compare" else "Touch and press here with your stylus",
            color = Pc.SlideLabel, fontSize = 12.sp, modifier = Modifier.padding(top = 3.dp, bottom = 10.dp),
        )
        PressureMeter("From pen", raw)
        Spacer(Modifier.height(9.dp))
        PressureMeter("After calibration", adjusted)
    }
}

@Composable
private fun PressureMeter(label: String, value: Float) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Pc.SlideLabel, fontSize = 12.sp, modifier = Modifier.weight(1f))
        Text("${(value * 100).roundToInt()}%", color = Pc.PanelTitle, fontSize = 12.sp)
    }
    Box(Modifier.fillMaxWidth().height(7.dp).clip(RoundedCornerShape(7.dp)).background(Pc.SlideTrack)) {
        Box(Modifier.fillMaxWidth(value.coerceIn(0f, 1f)).fillMaxHeight().background(Pc.Accent))
    }
}

/** Look up the global curve at the stylus pressure; the native renderer clips input at 1. */
internal fun calibratedPressure(raw: Float, samples: List<Float>): Float {
    if (samples.isEmpty() || !raw.isFinite()) return 0f
    val position = raw.coerceIn(0f, 1f) * (samples.size - 1)
    val index = position.toInt().coerceIn(0, samples.lastIndex)
    return (samples[index] +
        (samples[(index + 1).coerceAtMost(samples.lastIndex)] - samples[index]) * (position - index))
        .coerceIn(0f, 1f)
}
