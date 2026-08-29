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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.SafeImage
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.R
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

    // Saveable, so a rotation loses nothing typed but not yet saved. The
    // avatar stays out of it deliberately: it is image bytes, and saved
    // instance state is a Binder transaction with a hard size limit.
    var name by rememberSaveable { mutableStateOf(p.name() ?: "") }
    var email by rememberSaveable { mutableStateOf(p.email() ?: "") }
    var phone by rememberSaveable { mutableStateOf(p.phone() ?: "") }
    var signal by rememberSaveable { mutableStateOf(p.signal() ?: "") }
    var carModel by rememberSaveable { mutableStateOf(p.carModel() ?: "") }
    var carColor by rememberSaveable { mutableStateOf(p.carColor() ?: "") }
    var plate by rememberSaveable { mutableStateOf(p.plate() ?: "") }
    var pronouns by rememberSaveable { mutableStateOf(p.pronouns()) }
    var avatar by remember { mutableStateOf(p.avatar()) }
    var share by rememberSaveable { mutableStateOf(p.shareProfile()) }
    var saved by remember { mutableStateOf(false) }
    var avatarError by remember { mutableStateOf<String?>(null) }

    // The wire carries the code (§16.9, core's Pronouns enum); the labels are
    // presentation and follow the app language. Same order as the codes.
    val labels = androidx.compose.ui.res.stringArrayResource(R.array.pronoun_labels)

    val pick = rememberLauncherForActivityResult(
        ActivityResultContracts.GetContent()
    ) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        avatarError = null
        runCatching { squareThumbnail(context, uri) }
            .onSuccess { avatar = it; p.setAvatar(it); saved = false }
            .onFailure {
                avatarError = context.getString(R.string.myprofile_avatar_read_error)
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
                        SafeImage.fromBytes(it, SafeImage.AVATAR_PIXELS)
                    }
                }
                if (bmp != null) {
                    Image(
                        bmp.asImageBitmap(), stringResource(R.string.myprofile_your_picture),
                        Modifier.fillMaxSize(), contentScale = ContentScale.Crop,
                    )
                } else if (name.isNotBlank()) {
                    Text(
                        name.take(1).uppercase(),
                        fontSize = 28.sp, fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                } else {
                    Icon(Icons.Filled.AddAPhoto, stringResource(R.string.myprofile_add_picture))
                }
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.myprofile_your_picture), style = MaterialTheme.typography.titleSmall)
                Text(
                    // Said here because it is a real limit someone will hit: it
                    // has to fit in a record beside the keys, and a profile that
                    // does not fit is a contact who cannot be reached at all.
                    stringResource(R.string.myprofile_picture_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (avatar != null) {
                    TextButton(onClick = { avatar = null; p.setAvatar(null) }) { Text(stringResource(R.string.myprofile_remove)) }
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
            label = { Text(stringResource(R.string.myprofile_name_label)) },
            supportingText = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.myprofile_name_support),
                        Modifier.weight(1f),
                    )
                    CharCounter(name.length, 32)
                }
            },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(12.dp))
        Text(stringResource(R.string.myprofile_pronouns), style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(6.dp))
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
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
        // Said, like the driving note below says its half.
        //
        // Every other group on this screen explains itself — the picture, the
        // pronouns, the car — and the three that did not were the three that
        // matter most: an email, a phone and a Signal handle are the fields
        // that locate somebody off this app. They are scoped (see
        // MyProfile.toWire: they ride a deliberate contact exchange and
        // nothing else), which is exactly the sort of care nobody benefits
        // from unless it is written down where the typing happens.
        Text(
            stringResource(R.string.myprofile_reach_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(6.dp))
        Field(stringResource(R.string.myprofile_email), email,
            MyProfile.emailProblem(email)?.let { stringResource(it) }) { email = it; saved = false }
        Field(stringResource(R.string.myprofile_phone), phone,
            MyProfile.phoneProblem(phone)?.let { stringResource(it) },
            hint = stringResource(R.string.myprofile_phone_hint)) { phone = it; saved = false }
        Field(stringResource(R.string.myprofile_signal), signal,
            MyProfile.signalProblem(signal)?.let { stringResource(it) },
            hint = stringResource(R.string.myprofile_signal_hint)) { signal = it; saved = false }

        Spacer(Modifier.height(10.dp))
        Text(
            stringResource(R.string.myprofile_driving_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Field(stringResource(R.string.myprofile_car_model), carModel, null, hint = stringResource(R.string.myprofile_car_model_hint)) {
            carModel = it.take(24); saved = false
        }
        Field(stringResource(R.string.myprofile_car_colour), carColor, null, hint = stringResource(R.string.myprofile_car_colour_hint)) {
            carColor = it.take(16); saved = false
        }
        Field(stringResource(R.string.myprofile_plate), plate, null, hint = stringResource(R.string.myprofile_plate_hint)) {
            plate = it.take(12).uppercase(); saved = false
        }

        Spacer(Modifier.height(16.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = share, onCheckedChange = { share = it; p.setShareProfile(it) })
            Spacer(Modifier.width(12.dp))
            Column {
                Text(stringResource(R.string.myprofile_share_switch))
                Text(
                    if (share)
                        stringResource(R.string.myprofile_share_on_note)
                    else
                        stringResource(R.string.myprofile_share_off_note),
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
            Text(if (saved) stringResource(R.string.myprofile_saved) else stringResource(R.string.myprofile_save))
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
    // Picked from the gallery, which is also where anything shared into the
    // phone lands — and this one is on the way to becoming an avatar other
    // people's phones will decode.
    val src = SafeImage.fromStream(
        { context.contentResolver.openInputStream(uri) }, SafeImage.COMPOSE_PIXELS,
    ) ?: throw IllegalArgumentException("not an image")

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
