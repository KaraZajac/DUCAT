package org.ducatproject.ducat.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AddAPhoto
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.MyProfile
import java.io.ByteArrayOutputStream

/**
 * Everything this person publishes about themselves (§16.9).
 *
 * All of it optional except the name, all of it validated as it is typed
 * against the same rules `core` enforces on the wire — because being told at
 * the keyboard is the difference between a typo and a contact who silently
 * refuses your record.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun MyProfileEditor() {
    val context = LocalContext.current
    val p = remember { MyProfile(context) }

    var name by remember { mutableStateOf(p.name() ?: "") }
    var email by remember { mutableStateOf(p.email() ?: "") }
    var phone by remember { mutableStateOf(p.phone() ?: "") }
    var signal by remember { mutableStateOf(p.signal() ?: "") }
    var carModel by remember { mutableStateOf(p.carModel() ?: "") }
    var carColor by remember { mutableStateOf(p.carColor() ?: "") }
    var plate by remember { mutableStateOf(p.plate() ?: "") }
    var pronouns by remember { mutableStateOf(p.pronouns()) }
    var avatar by remember { mutableStateOf(p.avatar()) }
    var share by remember { mutableStateOf(p.shareProfile()) }
    var saved by remember { mutableStateOf(false) }
    var avatarError by remember { mutableStateOf<String?>(null) }

    val labels = remember { uniffi.ducat_mobile.pronounOptions() }

    val pick = rememberLauncherForActivityResult(
        ActivityResultContracts.GetContent()
    ) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        avatarError = null
        runCatching { squareThumbnail(context, uri) }
            .onSuccess { avatar = it; p.setAvatar(it); saved = false }
            .onFailure {
                avatarError = "Could not read that picture."
                DucatLog.w("Profile", "avatar: ${it.message}")
            }
    }

    Column(Modifier.fillMaxSize().padding(20.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(72.dp).clip(CircleShape)
                    .background(MaterialTheme.colorScheme.secondaryContainer)
                    .clickable { pick.launch("image/*") },
                contentAlignment = Alignment.Center,
            ) {
                val bmp = remember(avatar) {
                    avatar?.let {
                        runCatching { BitmapFactory.decodeByteArray(it, 0, it.size) }.getOrNull()
                    }
                }
                if (bmp != null) {
                    Image(
                        bmp.asImageBitmap(), "Your picture",
                        Modifier.fillMaxSize(), contentScale = ContentScale.Crop,
                    )
                } else if (name.isNotBlank()) {
                    Text(
                        name.take(1).uppercase(),
                        fontSize = 28.sp, fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                } else {
                    Icon(Icons.Filled.AddAPhoto, "Add a picture")
                }
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text("Your picture", style = MaterialTheme.typography.titleSmall)
                Text(
                    // Said here because it is a real limit someone will hit: it
                    // has to fit in a record beside the keys, and a profile that
                    // does not fit is a contact who cannot be reached at all.
                    "Shrunk to a thumbnail before it is sent — it travels in your " +
                        "contact record, which has room for a face and not a photo.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (avatar != null) {
                    TextButton(onClick = { avatar = null; p.setAvatar(null) }) { Text("Remove") }
                }
            }
        }
        avatarError?.let {
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }

        Spacer(Modifier.height(20.dp))
        OutlinedTextField(
            value = name,
            onValueChange = { if (it.length <= 32) { name = it; saved = false } },
            label = { Text("Name") },
            supportingText = {
                Text("Shown on cards you hand out. Whoever adds you can rename you.")
            },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(12.dp))
        Text("Pronouns", style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(6.dp))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            labels.forEachIndexed { i, label ->
                val code = i + 1
                FilterChip(
                    selected = pronouns == code,
                    // Tapping the selected one clears it. A closed list cannot
                    // hold everyone's pronouns, and someone who is not on it
                    // needs a way back to saying nothing — absence is a valid
                    // answer, not an unfinished form.
                    onClick = {
                        pronouns = if (pronouns == code) null else code
                        p.setPronouns(pronouns); saved = false
                    },
                    label = { Text(label) },
                )
            }
        }

        Spacer(Modifier.height(16.dp))
        Field("Email", email, MyProfile.emailProblem(email)) { email = it; saved = false }
        Field("Phone", phone, MyProfile.phoneProblem(phone),
            hint = "Digits only, country code included") { phone = it; saved = false }
        Field("Signal", signal, MyProfile.signalProblem(signal),
            hint = "name.12") { signal = it; saved = false }

        Spacer(Modifier.height(10.dp))
        Text(
            "Driving? Riders see this when you take their hail — it is how a " +
                "stranger finds the right car at the curb.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Field("Car model", carModel, null, hint = "Toyota Corolla") {
            carModel = it.take(24); saved = false
        }
        Field("Car colour", carColor, null, hint = "blue") {
            carColor = it.take(16); saved = false
        }
        Field("License plate", plate, null, hint = "KAR-4242") {
            plate = it.take(12).uppercase(); saved = false
        }

        Spacer(Modifier.height(16.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = share, onCheckedChange = { share = it; p.setShareProfile(it) })
            Spacer(Modifier.width(12.dp))
            Column {
                Text("Share this with new contacts")
                Text(
                    if (share)
                        "Everything above goes out when someone takes your card. " +
                            "Your Monero address has its own switch."
                    else
                        "Only your name travels. The rest stays on this device.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        Spacer(Modifier.height(20.dp))
        val problems = listOfNotNull(
            MyProfile.emailProblem(email),
            MyProfile.phoneProblem(phone),
            MyProfile.signalProblem(signal),
        )
        Button(
            onClick = {
                p.setName(name); p.setEmail(email); p.setPhone(phone); p.setSignal(signal)
                p.setCarModel(carModel); p.setCarColor(carColor); p.setPlate(plate)
                saved = true
            },
            enabled = problems.isEmpty(),
            modifier = Modifier.fillMaxWidth(),
        ) {
            if (saved) {
                Icon(Icons.Filled.Check, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
            }
            Text(if (saved) "Saved" else "Save")
        }
    }
}

@Composable
private fun Field(
    label: String,
    value: String,
    problem: String?,
    hint: String? = null,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        isError = problem != null,
        // Examples live inside the box as placeholders and leave when typing
        // starts; a permanent caption under every field read as clutter.
        // Below the box is reserved for actual problems.
        placeholder = { hint?.let { Text(it) } },
        supportingText = problem?.let { { Text(it) } },
        singleLine = true,
        modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp),
    )
}

/**
 * A small square JPEG, whatever came in.
 *
 * Re-encoded rather than passed through, and that is the security-relevant
 * part: what a photo picker hands back is an arbitrary file, and forwarding it
 * would mean publishing bytes this device never parsed to a decoder on someone
 * else's phone. Decoding and re-encoding means what goes out is something this
 * device's own image stack produced.
 */
private fun squareThumbnail(context: android.content.Context, uri: Uri): ByteArray {
    val src = context.contentResolver.openInputStream(uri).use { input ->
        BitmapFactory.decodeStream(input)
    } ?: throw IllegalArgumentException("not an image")

    val side = minOf(src.width, src.height)
    val cropped = Bitmap.createBitmap(
        src, (src.width - side) / 2, (src.height - side) / 2, side, side,
    )
    val scaled = Bitmap.createScaledBitmap(cropped, 128, 128, true)

    // Step the quality down until it fits. The bound is the protocol's, and a
    // picture that misses it is refused on arrival — better to lose detail here
    // than to publish a record nobody can read.
    for (q in intArrayOf(80, 65, 50, 35, 20)) {
        val out = ByteArrayOutputStream()
        scaled.compress(Bitmap.CompressFormat.JPEG, q, out)
        val bytes = out.toByteArray()
        if (bytes.size <= 12 * 1024) return bytes
    }
    throw IllegalArgumentException("could not shrink that picture enough")
}
