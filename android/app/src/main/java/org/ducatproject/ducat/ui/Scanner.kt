package org.ducatproject.ducat.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.ducatproject.ducat.DucatLog
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors
import androidx.compose.material.icons.filled.FlashlightOff
import androidx.compose.material.icons.filled.FlashlightOn
import androidx.compose.material.icons.Icons

/**
 * A QR scanner built on CameraX and the zxing decoder already in the app.
 *
 * **Not ML Kit.** ML Kit is a Google binary blob, and this app is meant for
 * F-Droid, whose inclusion policy makes that a correctness question rather than
 * a preference. zxing decodes the same codes and is already here for drawing
 * them.
 *
 * The analyzer reads the Y plane directly. A YUV frame's luminance is exactly
 * what a QR decoder wants, so converting to RGB first would cost a copy per
 * frame to throw away the colour.
 */
@Composable
fun QrScanner(
    prompt: String,
    onResult: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    // A Dialog, not a plain Surface. A caller renders this *before* the rest of
    // its screen, so a Surface sits in the same layout slot and whatever comes
    // after paints straight over it — the camera ran the whole time with the app
    // drawn on top of it. A dialog floats, and takes the back gesture with it.
    //
    // Which is exactly why there is a second entry point below: a caller that
    // already owns a full-screen dialog does **not** want another one floating
    // over its own chrome. Nesting them hid a screen's tab bar completely, and
    // the tabs looked like they had never been built.
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(
            usePlatformDefaultWidth = false,
            dismissOnBackPress = true,
        ),
    ) {
        Surface(Modifier.fillMaxSize()) {
            QrScannerContent(prompt, onResult, onDismiss)
        }
    }
}

/**
 * The scanner as ordinary content, for a screen that supplies its own frame.
 *
 * No dialog, no cancel row: whatever hosts this already has a way back, and a
 * second one is a second thing to explain.
 */
@Composable
fun QrScannerContent(
    prompt: String,
    onResult: (String) -> Unit,
    onDismiss: (() -> Unit)? = null,
) {
    val context = LocalContext.current
    // Tap or scan, one door: a card arriving over NFC takes the same path a
    // QR takes, so every screen that can scan can also be tapped against.
    NfcTapReader(onResult)
    var granted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    val ask = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted = it }

    LaunchedEffect(Unit) { if (!granted) ask.launch(Manifest.permission.CAMERA) }

    Column(Modifier.fillMaxSize()) {
            onDismiss?.let { cancel ->
                Row(
                    Modifier.fillMaxWidth().padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TextButton(onClick = cancel) { Text("Cancel") }
                    Spacer(Modifier.weight(1f))
                }
            }
            Text(
                prompt,
                Modifier.fillMaxWidth().padding(horizontal = 20.dp),
                textAlign = TextAlign.Center,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(12.dp))

            if (!granted) {
                Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text("The camera is not allowed", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Scanning needs it. Everything else in DUCAT works without it — " +
                            "a card can always be pasted as a link instead.",
                        textAlign = TextAlign.Center,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(16.dp))
                    Button(onClick = { ask.launch(Manifest.permission.CAMERA) }) { Text("Allow") }
                }
                return@Column
            }

            var failure by remember { mutableStateOf<String?>(null) }
            failure?.let {
                Text(
                    "The camera would not start: $it",
                    Modifier.padding(20.dp),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            CameraPreview(onResult) { failure = it }
    }
}

@Composable
private fun CameraPreview(onResult: (String) -> Unit, onFailure: (String) -> Unit) {
    // The torch, because codes get scanned where this app gets used: across a
    // bar. A scanner that cannot light the dark is a scanner that works in
    // demos.
    var camera by remember { mutableStateOf<androidx.camera.core.Camera?>(null) }
    var torch by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    // One frame is enough. Without this the callback fires repeatedly while the
    // code is still in view, and a scanner that reports the same card five times
    // makes the screen behind it decide five times.
    val done = remember { java.util.concurrent.atomic.AtomicBoolean(false) }
    val executor = remember { Executors.newSingleThreadExecutor() }

    DisposableEffect(Unit) { onDispose { executor.shutdown() } }

    Box(Modifier.fillMaxSize()) {
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            val view = PreviewView(ctx)
            val providerFuture = ProcessCameraProvider.getInstance(ctx)
            providerFuture.addListener({
                val provider = providerFuture.get()
                val preview = androidx.camera.core.Preview.Builder().build().also {
                    it.surfaceProvider = view.surfaceProvider
                }
                val analysis = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
                analysis.setAnalyzer(executor) { image ->
                    if (!done.get()) {
                        decode(image)?.let {
                            if (done.compareAndSet(false, true)) {
                                view.post { onResult(it) }
                            }
                        }
                    }
                    image.close()
                }
                // Reported rather than discarded. A swallowed bind failure is a
                // black rectangle with no explanation, which is exactly how the
                // last camera problem presented.
                runCatching {
                    provider.unbindAll()
                    camera = provider.bindToLifecycle(
                        lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis,
                    )
                }.onFailure { e ->
                    val msg = e.message ?: e.toString()
                    DucatLog.w("Scanner", "camera bind failed: $msg")
                    view.post { onFailure(msg) }
                }
            }, ContextCompat.getMainExecutor(ctx))
            view
        },
    )
    }
    // Over the preview, bottom corner — reachable with the thumb that is
    // already holding the phone up.
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomEnd) {
        FilledTonalIconButton(
            onClick = {
                torch = !torch
                camera?.cameraControl?.enableTorch(torch)
            },
            enabled = camera?.cameraInfo?.hasFlashUnit() == true,
            modifier = Modifier.padding(20.dp),
        ) {
            Icon(
                if (torch) Icons.Filled.FlashlightOff else Icons.Filled.FlashlightOn,
                if (torch) "Torch off" else "Torch on",
            )
        }
    }
}

private val reader = MultiFormatReader().apply {
    setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE)))
}

/** Decode one frame's luminance plane. Returns null when there is no code. */
private fun decode(image: ImageProxy): String? {
    val plane = image.planes.firstOrNull() ?: return null
    val buffer = plane.buffer
    val bytes = ByteArray(buffer.remaining()).also { buffer.get(it) }
    val source = PlanarYUVLuminanceSource(
        bytes, plane.rowStride, image.height,
        0, 0, plane.rowStride.coerceAtMost(image.width), image.height, false,
    )
    return try {
        reader.decodeWithState(BinaryBitmap(HybridBinarizer(source))).text
    } catch (_: Exception) {
        // Not finding a code is the normal case, once per frame.
        null
    } finally {
        reader.reset()
    }
}
