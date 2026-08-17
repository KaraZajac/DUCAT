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
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
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
        AccountsScreen()
    }
}

/** Everything a desk operator adjusts, in one place. */
@Composable
fun SettingsRoom() {
    val context = androidx.compose.ui.platform.LocalContext.current
    var tab by remember { mutableStateOf(0) }
    val tabs = listOf("Monero", "Network", "Logs", "Self-test")
    Column(Modifier.fillMaxSize()) {
        TabRow(selectedTabIndex = tab) {
            tabs.forEachIndexed { i, t ->
                Tab(selected = i == tab, onClick = { tab = i }, text = { Text(t) })
            }
        }
        Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(12.dp)) {
            when (tab) {
                0 -> MoneroPanel()
                1 -> NetworkPanel(storageDir = java.io.File(context.filesDir, "veilid").absolutePath)
                2 -> LogsScreen()
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
