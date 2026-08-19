package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.House
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Listings
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Stakes
import org.json.JSONArray
import org.json.JSONObject

/**
 * The renting mode (§15.11): what this device has to rent out.
 *
 * The owner's half of §16.18. Personal mode is where someone *looks* for a
 * car or a place; this is where someone *has* one. Same split as the taxi:
 * a rider hails from the wallet, a driver runs a shift from a mode.
 *
 * A listing is mostly a form, and the form is the design work — what a
 * stranger can search on has to be asked here, and what would let them walk
 * up to the door must be asked here too and then kept off the board.
 */
@Composable
fun RentingScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val listings = remember(version) { Listings.all(context) }
    var composing by remember { mutableStateOf<Int?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    composing?.let { kind ->
        ListingForm(kind = kind, onDone = { composing = null })
        return
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(16.dp)) {
            Button(
                onClick = { composing = Listings.KIND_PLACE },
                modifier = Modifier.weight(1f).height(48.dp),
            ) {
                Icon(Icons.Filled.House, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.rent_list_a_place))
            }
            Spacer(Modifier.width(12.dp))
            Button(
                onClick = { composing = Listings.KIND_VEHICLE },
                modifier = Modifier.weight(1f).height(48.dp),
            ) {
                Icon(Icons.Filled.DirectionsCar, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.rent_list_a_vehicle))
            }
        }
        error?.let {
            Text(
                it, Modifier.padding(horizontal = 16.dp),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        if (listings.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    stringResource(R.string.rent_nothing_listed),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            return
        }
        LazyColumn(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
            items(listings) { o ->
                MyListingCard(
                    o = o,
                    onPost = {
                        scope.launch {
                            error = null
                            withContext(Dispatchers.IO) {
                                runCatching { Listings.post(context, o.optString("id")) }
                            }.onFailure { error = moneyFailure(context, it) }
                        }
                    },
                    onStop = {
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                runCatching { Listings.unpost(context, o.optString("id")) }
                            }.onFailure { error = moneyFailure(context, it) }
                        }
                    },
                    onDelete = { Listings.remove(context, o.optString("id")) },
                )
                Spacer(Modifier.height(10.dp))
            }
        }
    }
}

@Composable
private fun MyListingCard(
    o: JSONObject,
    onPost: () -> Unit,
    onStop: () -> Unit,
    onDelete: () -> Unit,
) {
    val context = LocalContext.current
    val live = o.optString("board").isNotBlank()
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (o.optInt("kind") == Listings.KIND_VEHICLE) Icons.Filled.DirectionsCar
                    else Icons.Filled.House,
                    null, Modifier.size(18.dp),
                )
                Spacer(Modifier.width(8.dp))
                Text(o.optString("title"), style = MaterialTheme.typography.titleSmall)
            }
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(
                    R.string.rent_price_and_stake,
                    Amounts.show(context, o.optLong("pricePxmr")).primary,
                    Amounts.show(context, o.optLong("depositPxmr")).primary,
                ),
                style = MaterialTheme.typography.bodySmall,
            )
            Text(
                o.optString("area").ifBlank { stringResource(R.string.rent_no_area) },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(if (live) R.string.rent_live else R.string.rent_not_posted),
                style = MaterialTheme.typography.labelMedium,
                color = if (live) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Row {
                if (live) {
                    OutlinedButton(onClick = onStop) { Text(stringResource(R.string.rent_take_down)) }
                } else {
                    Button(onClick = onPost) { Text(stringResource(R.string.rent_post_it)) }
                    Spacer(Modifier.width(8.dp))
                    TextButton(onClick = onDelete) { Text(stringResource(R.string.rent_delete)) }
                }
            }
        }
    }
}

/**
 * The form.
 *
 * Split in two on the screen the way §16.18 splits it on the wire: what
 * strangers will see, and what only the person who books it will. The second
 * heading is not decoration — it is the only place a user is told that the
 * first half is public, at the moment they are typing into it.
 */
/** The form on its own, for rendering and review. */
@Composable
fun ListingFormPreview(kind: Int) = ListingForm(kind = kind, onDone = {})

@Composable
private fun ListingForm(kind: Int, onDone: () -> Unit) {
    val context = LocalContext.current
    val vehicle = kind == Listings.KIND_VEHICLE
    var title by remember { mutableStateOf("") }
    var area by remember { mutableStateOf("") }
    var price by remember { mutableStateOf("") }
    // Vehicle
    var make by remember { mutableStateOf("") }
    var model by remember { mutableStateOf("") }
    var year by remember { mutableStateOf("") }
    var color by remember { mutableStateOf("") }
    var seats by remember { mutableStateOf("") }
    var trim by remember { mutableStateOf("") }
    var gearbox by remember { mutableStateOf(2L) }
    var fuel by remember { mutableStateOf(1L) }
    // Place
    var rooms by remember { mutableStateOf("") }
    var sleeps by remember { mutableStateOf("") }
    var sizeM2 by remember { mutableStateOf("") }
    var whole by remember { mutableStateOf(true) }
    // Private
    var details by remember { mutableStateOf("") }
    var fix by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    // The Post button is dead without a fix — a listing has to sit on some
    // board, and which board is the question a position answers. So this
    // screen asks for the permission itself. It used to assume the answer was
    // already yes, and somebody who had never hailed a ride could fill in the
    // whole form and find the button greyed out under a sentence about
    // waiting for a fix that was never going to come.
    var asked by remember { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }
    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission(),
    ) { granted ->
        asked = true
        if (granted) attempt++
    }
    LaunchedEffect(attempt) {
        if (!locationAllowed(context)) {
            locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
            return@LaunchedEffect
        }
        grabFix(context) { fix = it }
    }

    // BigDecimal, like every other price in the app: a Double loses the
    // last piconero of a long figure and wraps silently on a huge one.
    val pricePxmr = Amounts.parse(price)?.let { Amounts.toPxmr(it) } ?: 0L
    val stake = Stakes.stakeFor(
        if (vehicle) Stakes.Deal.Vehicle else Stakes.Deal.Stay, pricePxmr,
    )

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text(
            stringResource(if (vehicle) R.string.rent_form_vehicle else R.string.rent_form_place),
            style = MaterialTheme.typography.titleLarge,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.rent_public_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = title, onValueChange = { title = it.take(60) },
            label = { Text(stringResource(R.string.rent_title)) },
            singleLine = true, modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = area, onValueChange = { area = it.take(40) },
            label = { Text(stringResource(R.string.rent_area)) },
            supportingText = { Text(stringResource(R.string.rent_area_hint)) },
            singleLine = true, modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = price,
            onValueChange = { price = it.filter { c -> Amounts.isNumberChar(c) } },
            label = {
                Text(stringResource(if (vehicle) R.string.rent_per_day else R.string.rent_per_night))
            },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            singleLine = true, modifier = Modifier.fillMaxWidth(),
        )
        if (stake > 0) {
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(
                    R.string.rent_stake_note,
                    Amounts.show(context, stake).primary,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Spacer(Modifier.height(16.dp))
        if (vehicle) {
            Row {
                OutlinedTextField(
                    value = make, onValueChange = { make = it.take(24) },
                    label = { Text(stringResource(R.string.rent_make)) },
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = model, onValueChange = { model = it.take(24) },
                    label = { Text(stringResource(R.string.rent_model)) },
                    singleLine = true, modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(8.dp))
            Row {
                OutlinedTextField(
                    value = year, onValueChange = { year = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(4) },
                    label = { Text(stringResource(R.string.rent_year)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = color, onValueChange = { color = it.take(24) },
                    label = { Text(stringResource(R.string.rent_color)) },
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = seats, onValueChange = { seats = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(2) },
                    label = { Text(stringResource(R.string.rent_seats)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = trim, onValueChange = { trim = it.take(24) },
                label = { Text(stringResource(R.string.rent_trim)) },
                supportingText = { Text(stringResource(R.string.rent_trim_hint)) },
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                FilterChip(
                    selected = gearbox == 1L, onClick = { gearbox = 1L },
                    label = { Text(stringResource(R.string.rent_manual)) },
                )
                Spacer(Modifier.width(8.dp))
                FilterChip(
                    selected = gearbox == 2L, onClick = { gearbox = 2L },
                    label = { Text(stringResource(R.string.rent_automatic)) },
                )
            }
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                listOf(
                    1L to R.string.rent_petrol, 2L to R.string.rent_diesel,
                    3L to R.string.rent_electric, 4L to R.string.rent_hybrid,
                ).forEach { (v, label) ->
                    FilterChip(
                        selected = fuel == v, onClick = { fuel = v },
                        label = { Text(stringResource(label)) },
                    )
                    Spacer(Modifier.width(6.dp))
                }
            }
        } else {
            Row {
                OutlinedTextField(
                    value = rooms, onValueChange = { rooms = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(2) },
                    label = { Text(stringResource(R.string.rent_rooms)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = sleeps, onValueChange = { sleeps = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(2) },
                    label = { Text(stringResource(R.string.rent_sleeps)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = sizeM2, onValueChange = { sizeM2 = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(4) },
                    label = { Text(stringResource(R.string.rent_size)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                FilterChip(
                    selected = whole, onClick = { whole = true },
                    label = { Text(stringResource(R.string.rent_whole_place)) },
                )
                Spacer(Modifier.width(8.dp))
                FilterChip(
                    selected = !whole, onClick = { whole = false },
                    label = { Text(stringResource(R.string.rent_private_room)) },
                )
            }
        }

        Spacer(Modifier.height(24.dp))
        HorizontalDivider()
        Spacer(Modifier.height(12.dp))
        Text(
            stringResource(R.string.rent_private_heading),
            style = MaterialTheme.typography.titleSmall,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.rent_private_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = details, onValueChange = { details = it.take(600) },
            label = { Text(stringResource(R.string.rent_private_label)) },
            minLines = 3, modifier = Modifier.fillMaxWidth(),
        )

        error?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
        Spacer(Modifier.height(16.dp))
        Row {
            Button(
                enabled = title.isNotBlank() && pricePxmr > 0 && fix != null,
                onClick = {
                    val here = fix ?: return@Button
                    val specs = JSONObject().apply {
                        if (vehicle) {
                            if (make.isNotBlank()) put("make", make)
                            if (model.isNotBlank()) put("model", model)
                            year.toLongOrNull()?.let { put("year", it) }
                            if (color.isNotBlank()) put("color", color)
                            if (trim.isNotBlank()) put("trim", trim)
                            seats.toLongOrNull()?.let { put("seats", it) }
                            put("gearbox", gearbox)
                            put("fuel", fuel)
                            put("subtype", 1L)
                        } else {
                            rooms.toLongOrNull()?.let { put("rooms", it) }
                            sleeps.toLongOrNull()?.let { put("sleeps", it) }
                            sizeM2.toLongOrNull()?.let { put("size_m2", it) }
                            put("subtype", if (whole) 1L else 2L)
                        }
                        put("features", JSONArray())
                    }
                    val draft = Listings.draft(
                        context, kind, title, area, pricePxmr,
                        here.first, here.second, specs, details,
                    )
                    Listings.put(context, draft)
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            runCatching { Listings.post(context, draft.optString("id")) }
                        }.onFailure { DucatLog.w("Renting", "post: ${it.message}") }
                        onDone()
                    }
                },
                modifier = Modifier.weight(1f).height(48.dp),
            ) { Text(stringResource(R.string.rent_post_it)) }
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = onDone) { Text(stringResource(R.string.rent_cancel)) }
        }
        if (fix == null) {
            val allowed = locationAllowed(context)
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(
                    if (allowed) R.string.rent_search_no_fix else R.string.rent_need_location,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            OutlinedButton(
                onClick = {
                    if (allowed) attempt++
                    else askForLocation(context, asked) { locPerm.launch(it) }
                },
            ) {
                Text(
                    stringResource(
                        if (allowed) R.string.rent_search_retry else R.string.rent_search_allow,
                    ),
                )
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
