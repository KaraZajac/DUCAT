package org.ducatproject.desk

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountBalanceWallet
import androidx.compose.material.icons.filled.Chat
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.LocalBar
import androidx.compose.material.icons.filled.PointOfSale
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material.icons.filled.LocalTaxi
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.QrCode
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ui.AccountsScreen
import org.ducatproject.ducat.ui.ActivityScreen
import org.ducatproject.ducat.ui.BalanceCard
import org.ducatproject.ducat.ui.BarTabScreen
import org.ducatproject.ducat.ui.BridgeSelfTest
import org.ducatproject.ducat.ui.ContactProfile
import org.ducatproject.ducat.ui.DonateScreen
import org.ducatproject.ducat.ui.LogsScreen
import org.ducatproject.ducat.ui.MoneroPanel
import org.ducatproject.ducat.ui.NetworkPanel
import org.ducatproject.ducat.ui.PosScreen

/**
 * The desk's rooms, and what is in them.
 *
 * Every screen behind these entries is the phone's own source compiled here
 * (see sharedLogic in build.gradle.kts) — the till really is the phone's
 * till, the activity list really is the phone's ledger, and a wording fixed
 * on one client is fixed on both. What the desk adds is the shape a desktop
 * wants: a rail down the side instead of a bottom bar, and rooms that stay
 * put instead of a stack you back out of.
 *
 * §15.11 says an operating mode is a whole app, and the phone honours that by
 * handing over its entire scaffold. A window is not a phone: here the modes
 * are rooms you walk between, which is the same claim — one thing at a time —
 * made in the idiom of a machine with a mouse.
 */
enum class Room(val label: String, val icon: ImageVector) {
    Conversations("Chat", Icons.Filled.Chat),
    Till("Till", Icons.Filled.PointOfSale),
    BarTab("Bar tab", Icons.Filled.LocalBar),
    Donate("Donations", Icons.Filled.Favorite),
    Activity("Activity", Icons.Filled.Receipt),
    Wallet("Wallet", Icons.Filled.AccountBalanceWallet),
    Ride("Ride", Icons.Filled.LocalTaxi),
    Codes("Codes", Icons.Filled.QrCode),
    Me("Me", Icons.Filled.Person),
    Settings("Settings", Icons.Filled.Settings),
}

@Composable
fun RoomRail(current: Room, unread: Int, onPick: (Room) -> Unit) {
    NavigationRail(Modifier.fillMaxHeight()) {
        Spacer(Modifier.height(8.dp))
        Room.entries.forEach { r ->
            NavigationRailItem(
                selected = r == current,
                onClick = { onPick(r) },
                icon = {
                    if (r == Room.Conversations && unread > 0) {
                        BadgedBox(badge = { Badge { Text("$unread") } }) {
                            Icon(r.icon, contentDescription = r.label)
                        }
                    } else {
                        Icon(r.icon, contentDescription = r.label)
                    }
                },
                label = { Text(r.label, style = MaterialTheme.typography.labelSmall) },
            )
        }
    }
}

/**
 * The wallet room: the phone's balance card over the phone's accounts list.
 *
 * The card's inputs are computed exactly as the phone's home screen computes
 * them — §17.2's capacity arithmetic through the bridge, not a Kotlin
 * re-derivation, which is the drift §18.12 exists to catch.
 */
@Composable
fun WalletRoom(onTopUp: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version by org.ducatproject.ducat.ContactStore.changes.collectAsState()
    val b = remember(version) { org.ducatproject.ducat.Wallet.balances(context) }
    val approx = remember(b.spendableOutputs) {
        uniffi.ducat_mobile.approxPaymentsSupported(b.spendableOutputs.toUInt()).toInt()
    }
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        BalanceCard(
            spendablePxmr = b.spendablePxmr,
            capacity = org.ducatproject.ducat.Capacity(approxPayments = approx),
            float = org.ducatproject.ducat.Float(
                spendablePxmr = b.spendablePxmr,
                lockedPxmr = b.lockedPxmr,
                blocksToUnlock = b.blocksToUnlock.toInt(),
                unlockedOutputs = b.spendableOutputs,
            ),
            locked = org.ducatproject.ducat.Money(
                b.lockedPxmr / 1_000_000L, symbol = "", exponent = 6,
            ),
            onTopUp = onTopUp,
            sync = b,
        )
        Spacer(Modifier.height(16.dp))
        // Its own scrolling list: it must not be inside another scroller,
        // which measures with unbounded height and throws.
        Box(Modifier.weight(1f)) { AccountsScreen() }
    }
}

/**
 * Hailing and driving, from a machine that does not move.
 *
 * The rider half needs a position, and a desk has one the moment its
 * operator types it (Settings → Where this desk is). Everything downstream
 * is the phone's: the same geocells, the same claim-once boards, the same
 * offer ceremony. The driver half is here for completeness and for the
 * standing-arbiter case; a person actually driving wants the phone.
 */
@Composable
fun RideRoom() {
    val context = androidx.compose.ui.platform.LocalContext.current
    var driving by remember { mutableStateOf(false) }
    val placed = remember { org.ducatproject.ducat.ui.DeskLocation.get(context) != null }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(if (driving) "Driving" else "Ride", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            TextButton(onClick = { driving = !driving }) {
                Text(if (driving) "I need a ride" else "I am driving")
            }
        }
        HorizontalDivider()
        if (!placed) {
            Column(Modifier.padding(16.dp)) {
                Text(
                    "This desk does not know where it is yet.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text(
                    "A phone reads its GPS; a desk is told once — Settings, " +
                        "then \"Where this desk is\". Boards are ~1.2 km coarse, " +
                        "so the nearest corner is precise enough.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        Box(Modifier.fillMaxSize()) {
            if (driving) org.ducatproject.ducat.ui.DriveScreen()
            else org.ducatproject.ducat.ui.TaxiScreen()
        }
    }
}

/** Who this desk is, to everyone it hands a card. */
@Composable
fun MeRoom() {
    Box(Modifier.fillMaxSize()) { org.ducatproject.ducat.ui.MyProfileEditor() }
}

/** Everything a desk operator adjusts, in one place. */
@Composable
fun SettingsRoom() {
    val context = androidx.compose.ui.platform.LocalContext.current
    var tab by remember { mutableStateOf(0) }
    val tabs = listOf("General", "Where", "Backup", "Monero", "Network", "Logs", "Self-test")
    Column(Modifier.fillMaxSize()) {
        TabRow(selectedTabIndex = tab) {
            tabs.forEachIndexed { i, t ->
                Tab(selected = i == tab, onClick = { tab = i }, text = { Text(t) })
            }
        }
        Box(Modifier.fillMaxSize().padding(12.dp)) {
            when (tab) {
                // The phone's own settings: language, currency, units, theme.
                0 -> org.ducatproject.ducat.ui.SettingsScreen(
                    themeMode = org.ducatproject.ducat.ui.ThemeMode.Mocha,
                    onThemeChange = {},
                )
                1 -> DeskPlaceSetting()
                2 -> org.ducatproject.ducat.ui.BackupSettings(
                    spendKeyHex = org.ducatproject.ducat.WalletStore(context).spendKeyHex(),
                    restoreHeight = org.ducatproject.ducat.WalletStore(context).restoreHeight(),
                    personaSecret = org.ducatproject.ducat.PersonaStore(context).secret(),
                )
                3 -> MoneroPanel()
                4 -> NetworkPanel(storageDir = java.io.File(context.filesDir, "veilid").absolutePath)
                5 -> LogsScreen()
                else -> BridgeSelfTest()
            }
        }
    }
}

/** A contact's profile, hosted rather than pushed onto a back stack. */
@Composable
fun ProfileDialog(contact: Contact, onOpenChat: (Contact) -> Unit, onClose: () -> Unit) {
    androidx.compose.ui.window.Dialog(
        onDismissRequest = onClose,
        properties = org.ducatproject.ducat.ui.fullScreenDialogProperties(),
    ) {
        Surface(
            Modifier.width(560.dp).fillMaxHeight(0.85f),
            color = MaterialTheme.colorScheme.background,
        ) {
            ContactProfile(contact = contact, onBack = onClose, onOpenChat = onOpenChat)
        }
    }
}

/** A room that fills the window, with the phone screen inside it. */
@Composable
fun RoomHost(title: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(title, style = MaterialTheme.typography.titleMedium)
        }
        HorizontalDivider()
        Box(Modifier.fillMaxSize()) { content() }
    }
}

@Composable
fun TillRoom() = RoomHost("Till") { PosScreen() }

@Composable
fun BarTabRoom() = RoomHost("Bar tab") { BarTabScreen() }

@Composable
fun DonateRoom() = RoomHost("Donations") { DonateScreen() }

@Composable
fun ActivityRoom() = RoomHost("Activity") { ActivityScreen() }


/**
 * Where this desk is — the one thing a phone reads from a satellite and a
 * desk has to be told. Typed once, it makes the geocell features work here
 * exactly as they do on a phone; left empty, every one of them reports "no
 * fix", which is what a phone indoors reports too.
 */
@Composable
fun DeskPlaceSetting() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val current = remember { org.ducatproject.ducat.ui.DeskLocation.get(context) }
    var text by remember {
        mutableStateOf(current?.let { org.ducatproject.ducat.ui.DeskLocation.format(it) } ?: "")
    }
    var saved by remember { mutableStateOf<String?>(null) }
    Column(Modifier.padding(4.dp)) {
        Text("Where this desk is", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(
            "Latitude, longitude. Boards are about 1.2 km across, so the " +
                "nearest street corner is as precise as this needs to be — and " +
                "it never leaves this machine except as the coarse cell a hail " +
                "is posted to.",
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = text,
            onValueChange = { text = it; saved = null },
            singleLine = true,
            placeholder = { Text("52.5200, 13.4050") },
        )
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(onClick = {
                val fix = org.ducatproject.ducat.ui.DeskLocation.parse(text)
                if (fix == null) {
                    saved = "That is not a pair of coordinates."
                } else {
                    org.ducatproject.ducat.ui.DeskLocation.set(context, fix.first, fix.second)
                    saved = "Saved."
                }
            }) { Text("Save") }
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = {
                org.ducatproject.ducat.ui.DeskLocation.clear(context)
                text = ""
                saved = "Cleared — this desk has no position again."
            }) { Text("Forget") }
            Spacer(Modifier.width(12.dp))
            saved?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        }
    }
}

/** The phone's code hub: this desk's card, and a place to paste one. */
@Composable
fun CodesRoom(onOpenChat: (Contact) -> Unit, onScanAddress: (String, Long) -> Unit) {
    org.ducatproject.ducat.ui.QrHub(
        onOpenChat = onOpenChat,
        onScanAddress = onScanAddress,
        onClose = {},
    )
}


/**
 * §4.3, enforced on the desk as the phone enforces it.
 *
 * The phone will not show a funded wallet until its backup step is done —
 * `onboarded` stays false until then. The desk used to mint a wallet
 * silently at first launch, which meant a shopkeeper could take payments
 * into a key nobody had exported and one disk failure would end it.
 *
 * The wallet is still created at startup, exactly as onboarding creates it,
 * because the backup screen needs a key to show. What waits is *everything
 * else*: until a backup has actually been exported, this is the only screen
 * the desk has. And it is a real export, not a promise — BackupSettings
 * records one (`markBackupExported`), and the button below reads that record
 * rather than the operator's word for it.
 */
@Composable
fun FirstRun(onDone: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version by org.ducatproject.ducat.ContactStore.changes.collectAsState()
    val exported = remember(version) {
        org.ducatproject.ducat.ContactStore(context).backupExportedAt() > 0L
    }
    val wallet = remember(version) { org.ducatproject.ducat.WalletStore(context) }
    Column(Modifier.fillMaxSize().padding(24.dp)) {
        Text("Before this desk takes money", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        Text(
            "This desk has its own wallet and its own identity — the keys are " +
                "on this machine and nowhere else. There is no operator to ask " +
                "for them back, so a backup is not housekeeping: it is the only " +
                "copy that will ever exist.",
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            "Export it below, keep it somewhere that survives this machine, " +
                "then this desk opens.",
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(16.dp))
        // The same explanation the phone gives before a first deal, from the
        // same strings — a desk operator is exactly as entitled to know how
        // trust works here, and more likely to be the one asked about it.
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp)) {
                Text(
                    androidx.compose.ui.res.stringResource(
                        org.ducatproject.ducat.R.string.onb_trust_title,
                    ),
                    style = MaterialTheme.typography.titleSmall,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    androidx.compose.ui.res.stringResource(
                        org.ducatproject.ducat.R.string.onb_trust_body,
                        org.ducatproject.ducat.Stakes.Deal.Ride.percent,
                        org.ducatproject.ducat.Stakes.Deal.Stay.percent,
                        org.ducatproject.ducat.Stakes.Deal.Vehicle.percent,
                    ),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        Spacer(Modifier.height(16.dp))
        Box(Modifier.weight(1f)) {
            org.ducatproject.ducat.ui.BackupSettings(
                spendKeyHex = wallet.spendKeyHex(),
                restoreHeight = wallet.restoreHeight(),
                personaSecret = org.ducatproject.ducat.PersonaStore(context).secret(),
            )
        }
        Spacer(Modifier.height(12.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(enabled = exported, onClick = onDone) { Text("Open the desk") }
            Spacer(Modifier.width(12.dp))
            Text(
                if (exported) "Backup exported — keep it safe."
                else "Waiting for a backup to be exported.",
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}


/**
 * The lock screen: a desk whose vault exists but is closed.
 *
 * There is nothing behind this — the stores cannot be read without the key,
 * so this is not a screen guarding a door, it is the door.
 */
@Composable
fun UnlockScreen(dir: java.io.File, onUnlocked: () -> Unit) {
    var pass by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(Modifier.width(420.dp)) {
            Text("This desk is locked", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.height(8.dp))
            Text(
                "Its keys are encrypted on disk. The passphrase is the only " +
                    "thing that opens them — it is not stored anywhere, here " +
                    "or elsewhere.",
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = pass,
                onValueChange = { pass = it; error = null },
                singleLine = true,
                label = { Text("Passphrase") },
                visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
                modifier = Modifier.fillMaxWidth(),
            )
            error?.let {
                Spacer(Modifier.height(6.dp))
                Text(it, color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(12.dp))
            Button(
                enabled = !busy && pass.isNotEmpty(),
                onClick = {
                    busy = true
                    // Argon2id at 64 MiB is meant to take a moment; off the
                    // frame thread so the window does not appear to hang.
                    Thread {
                        val r = org.ducatproject.ducat.DeskVault.unlock(dir, pass)
                        busy = false
                        if (r.isSuccess) onUnlocked()
                        else error = "That passphrase does not open this desk."
                    }.start()
                },
            ) { Text(if (busy) "Opening…" else "Unlock") }
        }
    }
}

/**
 * Choosing a passphrase, on a desk that has none.
 *
 * Declining is allowed and says what it costs, because a desk that refuses to
 * run without one would strand every desk that already exists — and a warning
 * someone read beats a lock they worked around.
 */
@Composable
fun ProtectStep(dir: java.io.File, onSettled: () -> Unit) {
    var pass by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().padding(24.dp)) {
        Text("Lock this desk's keys", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        Text(
            "A phone keeps its keys in hardware this machine does not have, so " +
                "the desk encrypts them with a passphrase you choose. You will " +
                "be asked for it each time the desk starts.",
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            "It is not stored anywhere and cannot be recovered — the backup you " +
                "export next is what survives a forgotten one.",
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(16.dp))
        OutlinedTextField(
            value = pass,
            onValueChange = { pass = it; error = null },
            singleLine = true,
            label = { Text("Passphrase") },
            visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = again,
            onValueChange = { again = it; error = null },
            singleLine = true,
            label = { Text("Again") },
            visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
        )
        error?.let {
            Spacer(Modifier.height(6.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
        Spacer(Modifier.height(16.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                enabled = !busy && pass.length >= 8 && pass == again,
                onClick = {
                    busy = true
                    Thread {
                        val r = org.ducatproject.ducat.DeskVault.create(dir, pass)
                        busy = false
                        if (r.isSuccess) onSettled()
                        else error = r.exceptionOrNull()?.message ?: "Could not lock this desk."
                    }.start()
                },
            ) { Text(if (busy) "Locking…" else "Use this passphrase") }
            Spacer(Modifier.width(12.dp))
            TextButton(onClick = onSettled) { Text("Not now") }
            Spacer(Modifier.width(12.dp))
            Text(
                when {
                    pass.isNotEmpty() && pass.length < 8 -> "At least eight characters."
                    pass.isNotEmpty() && pass != again -> "The two do not match."
                    else -> "Without one, the keys stay readable to anything that can read your home directory."
                },
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}
