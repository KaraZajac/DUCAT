package org.ducatproject.ducat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.ui.BalanceCard
import uniffi.ducat_mobile.approxPaymentsSupported
import uniffi.ducat_mobile.protocolVersion

/**
 * One activity, one app.
 *
 * The navigation follows PayPal's — a hamburger for the long tail, a bottom bar
 * with an elevated centre action — because that shape is familiar to hundreds of
 * millions of people and there is no advantage in being novel about navigation.
 *
 * The centre action is **Send / Request**, and that is not a UI invention: it is
 * §15.2's `presenter_role`. Request means *I present and you tap me* (the POS
 * direction); Send means *I read your tap*. Both run end to end, and they are
 * **not symmetric** — the presenter supplies reachability, so the reader drives
 * every round trip. A screen written against one and reused for the other hangs.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { MaterialTheme { DucatApp() } }
    }
}

enum class Tab(val label: String) { Home("Home"), Accounts("Accounts"), Activity("Activity"), Menu("Menu") }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DucatApp() {
    var tab by remember { mutableStateOf(Tab.Home) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (tab == Tab.Home) "" else tab.label) },
                navigationIcon = {
                    IconButton(onClick = { tab = Tab.Menu }) {
                        Icon(Icons.Filled.Menu, contentDescription = "Menu")
                    }
                },
            )
        },
        floatingActionButton = {
            // The one verb that dominates, raised the way PayPal raises theirs.
            ExtendedFloatingActionButton(
                onClick = { /* tap flow */ },
                icon = { Icon(Icons.Filled.SwapVert, contentDescription = null) },
                text = { Text("Send / Request") },
            )
        },
        floatingActionButtonPosition = FabPosition.Center,
        bottomBar = {
            NavigationBar {
                listOf(
                    Tab.Home to Icons.Filled.Home,
                    Tab.Accounts to Icons.Filled.AccountBalanceWallet,
                    Tab.Activity to Icons.Filled.Receipt,
                    Tab.Menu to Icons.Filled.Settings,
                ).forEach { (t, icon) ->
                    NavigationBarItem(
                        selected = tab == t,
                        onClick = { tab = t },
                        icon = { Icon(icon, contentDescription = t.label) },
                        label = { Text(t.label) },
                    )
                }
            }
        },
    ) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
        ) {
            when (tab) {
                Tab.Home -> HomeScreen()
                Tab.Accounts -> Placeholder(
                    "Accounts",
                    "Float, reserve and bond — §17.2 forbids showing them as one number."
                )
                Tab.Activity -> Placeholder(
                    "Activity",
                    "Mostly nameless by design: a name exists only where §16.3's " +
                        "post-receipt coda established a contact."
                )
                Tab.Menu -> Placeholder(
                    "Menu",
                    "Personas, backup, custody, verification thresholds, markets, relays, records.\n\n" +
                        // Proof the native core is loaded and answering, from the
                        // one call that cannot be faked by a stub.
                        "speaking ${protocolVersion()}"
                )
            }
        }
    }
}

@Composable
private fun HomeScreen() {
    // The wallet figures are still placeholders — they arrive from a Monero
    // wallet, which is the next piece. **The capacity is not a placeholder.**
    // It comes from `core::float` across the bridge, so the one number §17.2
    // forbids overstating is computed by the same code the conformance vectors
    // and the harness run, rather than by a second implementation in Kotlin.
    val float = Float(
        spendablePxmr = 40_000_000_000,
        lockedPxmr = 12_000_000_000,
        blocksToUnlock = 7,
        unlockedOutputs = 4,
    )
    val approx = remember(float.unlockedOutputs) {
        approxPaymentsSupported(float.unlockedOutputs.toUInt()).toInt()
    }
    BalanceCard(
        spendable = Money(4000),
        capacity = Capacity(approxPayments = approx),
        float = float,
        locked = Money(1200),
        onTopUp = {},
    )
}

@Composable
private fun Placeholder(title: String, note: String) {
    Column(
        Modifier.fillMaxWidth().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        Text(note, style = MaterialTheme.typography.bodyMedium)
    }
}
