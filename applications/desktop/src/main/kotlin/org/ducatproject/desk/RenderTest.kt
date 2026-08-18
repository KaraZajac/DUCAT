package org.ducatproject.desk

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Density
import java.io.File

/**
 * Every hosted screen, rendered off-screen and looked at.
 *
 * Compiling proves a screen's types line up; it does not prove the screen
 * *draws*. A missing resource, a shim that throws on first composition, a
 * layout that collapses to nothing — all compile perfectly and all produce a
 * blank rectangle in front of a user. This renders each one into a bitmap
 * with no display attached and asserts the pixels are not all one colour,
 * which is the cheapest question worth asking: did anything appear?
 *
 * `./gradlew :desktop:rendertest` — writes the PNGs beside the report so a
 * human can also just look.
 */
@OptIn(ExperimentalComposeUiApi::class)
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("RENDER_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val out = File(dir, "renders").apply { mkdirs() }
    val context = DeskContext(dir)
    android.res.DeskRes.setLocale("en")

    var failures = 0

    fun render(name: String, w: Int = 1100, h: Int = 800, content: @Composable () -> Unit) {
        val scene = ImageComposeScene(width = w, height = h, density = Density(1f))
        val bytes = try {
            scene.setContent {
                CompositionLocalProvider(LocalContext provides context) {
                    MaterialTheme(colorScheme = darkColorScheme()) {
                        Surface(Modifier.fillMaxSize()) {
                            Box(Modifier.fillMaxSize()) { content() }
                        }
                    }
                }
            }
                // One frame is enough to answer "did it draw"; a screen that
                // needs the network before it shows anything is a screen that
                // shows nothing on a slow morning, which is its own bug.
                val img = scene.render()
                img.encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)?.bytes
            } catch (t: Throwable) {
                println("RENDER FAIL $name — ${t::class.simpleName}: ${t.message}")
                failures++
                null
            } finally {
                runCatching { scene.close() }
            }
        if (bytes == null || bytes.isEmpty()) {
            if (bytes != null) { println("RENDER FAIL $name — encoded to nothing"); failures++ }
            return
        }
        File(out, "$name.png").writeBytes(bytes)
        val img = javax.imageio.ImageIO.read(java.io.ByteArrayInputStream(bytes))
        // Distinct colours: a blank screen has one, anything drawn has more.
        val colours = HashSet<Int>()
        var y = 0
        while (y < img.height && colours.size <= 40) {
            var x = 0
            while (x < img.width && colours.size <= 40) {
                colours.add(img.getRGB(x, y)); x += 7
            }
            y += 7
        }
        val ok = colours.size >= 3
        println(
            "RENDER ${if (ok) "ok  " else "FAIL"} $name — ${bytes.size} bytes, " +
                "${colours.size} distinct colours",
        )
        if (!ok) failures++
    }

    // The rooms, as the window hosts them.
    render("activity") { org.ducatproject.ducat.ui.ActivityScreen() }
    render("till") { org.ducatproject.ducat.ui.PosScreen() }
    render("bartab") { org.ducatproject.ducat.ui.BarTabScreen() }
    render("donate") { org.ducatproject.ducat.ui.DonateScreen() }
    render("wallet") { WalletRoom(onTopUp = {}) }
    render("settings") { SettingsRoom() }
    render("me") { MeRoom() }
    render("ride") { RideRoom() }
    render("accounts") { org.ducatproject.ducat.ui.AccountsScreen() }
    render("monero") { org.ducatproject.ducat.ui.MoneroPanel() }
    render("logs") { org.ducatproject.ducat.ui.LogsScreen() }
    render("selftest") { org.ducatproject.ducat.ui.BridgeSelfTest() }
    render("chatlist") {
        org.ducatproject.ducat.ui.ChatListScreen(personaSecret = null, onOpenChat = {})
    }
    render("scanner") {
        org.ducatproject.ducat.ui.QrScannerContent("Scan a card", onResult = {})
    }
    render("place") { DeskPlaceSetting() }
    // The sentence the whole trust model rests on, as a user meets it.
    // The moment a rider decides: does the screen tell them the whole cost?
    render("ride-offer", w = 480, h = 900) {
        org.ducatproject.ducat.ui.RideOfferScreen(
            m = org.ducatproject.ducat.StoredMessage(
                outgoing = false, seq = 3, body = "Corner of Oak and 5th",
                timestamp = System.currentTimeMillis() / 1000,
                kind = 6, amountPxmr = 60_000_000_000L, etaSecs = 240L,
            ),
            contact = org.ducatproject.ducat.Contact(
                personaHex = "00".repeat(32),
                petname = null,
                assertedName = "Sam",
                myOutbox = "",
                theirOutbox = "",
            ),
            onAccept = {}, onDecline = {}, onClose = {},
        )
    }
    render("onboarding-trust", w = 900, h = 700) {
        org.ducatproject.ducat.ui.OnboardingFlow(
            state = org.ducatproject.ducat.ui.Onboarding(
                step = org.ducatproject.ducat.ui.Step.Trust,
            ),
            onState = {},
        )
    }
    render("firstrun") { FirstRun(onDone = {}) }
    render("protect") { ProtectStep(dir, onSettled = {}) }
    render("unlock") { UnlockScreen(dir, onUnlocked = {}) }
    render("phone-settings") {
        org.ducatproject.ducat.ui.SettingsScreen(
            themeMode = org.ducatproject.ducat.ui.ThemeMode.Mocha,
            onThemeChange = {},
        )
    }
    render("backup") {
        val ctx = context
        org.ducatproject.ducat.ui.BackupSettings(
            spendKeyHex = org.ducatproject.ducat.WalletStore(ctx).spendKeyHex(),
            restoreHeight = org.ducatproject.ducat.WalletStore(ctx).restoreHeight(),
            personaSecret = org.ducatproject.ducat.PersonaStore(ctx).secret(),
        )
    }
    render("network") {
        org.ducatproject.ducat.ui.NetworkPanel(
            storageDir = File(context.filesDir, "veilid").absolutePath,
        )
    }
    render("route") {
        org.ducatproject.ducat.ui.RouteMap(
            from = 525200000L to 134050000L,
            to = 525500000L to 134400000L,
            route = listOf(
                525200000L to 134050000L,
                525300000L to 134200000L,
                525500000L to 134400000L,
            ),
            modifier = Modifier.fillMaxSize(),
        )
    }
    render("drivermap") {
        org.ducatproject.ducat.ui.DriverMap(
            me = 525200000L to 134050000L,
            fares = listOf((525400000L to 134300000L) to "USD 4.20"),
            onFareTap = {},
            coverage = longArrayOf(525000000L, 525800000L, 133800000L, 134600000L),
            modifier = Modifier.fillMaxSize(),
        )
    }

    println(
        if (failures == 0) "RENDERTEST OK — ${out.absolutePath}"
        else "RENDERTEST FAILED ($failures)",
    )
    if (failures > 0) kotlin.system.exitProcess(1)
}
