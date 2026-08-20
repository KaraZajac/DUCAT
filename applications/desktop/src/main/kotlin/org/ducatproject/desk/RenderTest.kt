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
    // The menu behind the till: empty here, which is the state a shop sees
    // before it has typed anything in and the one most likely to draw nothing.
    render("items", w = 900, h = 700) { org.ducatproject.ducat.ui.ItemsScreen() }
    // The counter, facing the other way. Empty catalogue, which is what a
    // shop sees before it has set anything up. The stocked counterparts of
    // these three are rendered at the end, after something has been put on
    // the menu — keep them there, or these stop being the empty states.
    render("kiosk", w = 520, h = 900) { org.ducatproject.ducat.ui.KioskScreen() }
    // The gate in front of every payment. Rendered in its set-a-PIN state,
    // which is what a device that has never had one shows.
    render("pin", w = 700, h = 700) {
        org.ducatproject.ducat.ui.PinGate(open = true, onDismiss = {}, onPassed = {})
    }
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
    // The one list of people that had no faces on it. Seeded, because the
    // empty state is not what this change was about.
    run {
        val store = org.ducatproject.ducat.ContactStore(context)
        if (store.all().isEmpty()) {
            listOf(
                "sh3ll3t0r" to "15e9b2cfce73c7b6fc4e0d3e" + "11".repeat(20),
                ".kara" to "894f2ee09e29dee2a107a859" + "22".repeat(20),
                "desktop" to "5b6489c9c7fd0dcf50545e7c" + "33".repeat(20),
            ).forEach { (name, hex) ->
                store.add(
                    org.ducatproject.ducat.Contact(
                        personaHex = hex, petname = name, assertedName = name,
                        myOutbox = "VLD0:mine", theirOutbox = "VLD0:theirs",
                    ),
                )
            }
        }
    }
    render("contacts", w = 520, h = 900) {
        org.ducatproject.ducat.ui.ContactsAdminPreview()
    }
    render("chatlist") {
        org.ducatproject.ducat.ui.ChatListScreen(personaSecret = null, onOpenChat = {})
    }
    render("scanner") {
        org.ducatproject.ducat.ui.QrScannerContent("Scan a card", onResult = {})
    }
    render("place") { DeskPlaceSetting() }
    // The owner's side: what someone with a car or a room fills in.
    render("renting-mode", w = 520, h = 900) { org.ducatproject.ducat.ui.RentingScreen() }
    // The seeker's side: the two chips on the personal Home screen.
    render("rent-form-car", w = 520, h = 1400) {
        org.ducatproject.ducat.ui.ListingFormPreview(kind = 2)
    }
    render("rent-form-place", w = 520, h = 1200) {
        org.ducatproject.ducat.ui.ListingFormPreview(kind = 1)
    }
    // The three ways into the personal screen's occasional jobs. This used to
    // render RentSearchCard's pair of chips; when those became tiles the card
    // was left drawing nothing at all, and this test said so — one colour.
    render("home-tiles", w = 520, h = 220) {
        org.ducatproject.ducat.ui.HomeTiles(
            onHail = {}, onRentCar = {}, onRentPlace = {},
        )
    }
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
    // The escrow moments, which used to be three lines in a banner strip. A
    // screen that asks for money it cannot unsend should look like one.
    render("escrow-step", w = 480, h = 900) {
        org.ducatproject.ducat.ui.EscrowStep(
            contact = org.ducatproject.ducat.Contact(
                personaHex = "11".repeat(32),
                petname = null,
                assertedName = "Sam",
                myOutbox = "",
                theirOutbox = "",
            ),
            title = "Escrow ready — the fare goes in before the ride.",
            amountPxmr = 1_200_000_000L,
            note = "0.000400 XMR of that is your stake — it comes back when this is finished.",
            action = "Secure fare (0.001200 XMR)",
            onAction = {},
            onClose = {},
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

    // The counter, which had never been drawn anywhere. A saved menu wants
    // something on it before the picker is worth a picture, so give it two
    // things and a rate to price them with.
    run {
        val ctx = context
        org.ducatproject.ducat.RateStore(ctx)
            .store(150.0, System.currentTimeMillis() / 1000, "rendertest")
        if (org.ducatproject.ducat.Catalogue.live(ctx).isEmpty()) {
            org.ducatproject.ducat.Catalogue.put(
                ctx, org.ducatproject.ducat.Catalogue.draft(ctx, "Flat white", "3.20"),
            )
            org.ducatproject.ducat.Catalogue.put(
                ctx, org.ducatproject.ducat.Catalogue.draft(ctx, "Croissant", "2.50"),
            )
        }
    }
    render("items-stocked", w = 900, h = 700) { org.ducatproject.ducat.ui.ItemsScreen() }
    render("item-picker", w = 520, h = 300) {
        org.ducatproject.ducat.ui.ItemPicker(onPick = { _, _ -> })
    }
    render("kiosk-stocked", w = 520, h = 900) { org.ducatproject.ducat.ui.KioskScreen() }
    // What the shop sees once it has proved it is the shop. The menu lives
    // here too now, so a stall can put the iced coffee on without leaving
    // kiosk mode and showing a customer its wallet on the way through.
    render("kiosk-staff", w = 520, h = 900) {
        org.ducatproject.ducat.ui.StaffPanelPreview(tab = 0)
    }
    render("kiosk-staff-menu", w = 520, h = 900) {
        org.ducatproject.ducat.ui.StaffPanelPreview(tab = 1)
    }
    // The first morning: a menu typed in, and a phone that has not reached
    // the network, so nothing can be converted into monero yet. Every chip
    // disables itself correctly; the line under them is what stops the seller
    // tapping at a dead till wondering what they did wrong.
    run {
        val ctx = context
        val rates = org.ducatproject.ducat.RateStore(ctx)
        val had = rates.cached()
        rates.store(0.0, 0L, "")
        render("kiosk-no-rate", w = 520, h = 900) { org.ducatproject.ducat.ui.KioskScreen() }
        had?.let { (v, at) -> rates.store(v, at, "rendertest") }
    }
    // The counter mid-sale. Both panels take their whole state as parameters,
    // so they draw here without a node behind them — which is the only way to
    // see the two screens a customer actually stands in front of.
    run {
        val ctx = context
        val basket = org.ducatproject.ducat.Catalogue.live(ctx).mapNotNull { item ->
            org.ducatproject.ducat.Catalogue.price(ctx, item).getOrNull()?.let {
                org.ducatproject.ducat.BillItem(item.name, it.pxmr)
            }
        }
        val order = org.ducatproject.ducat.Orders.begin(ctx, basket)
        // Waiting to be tapped or scanned. A real card URI, so the QR is the
        // size and density a phone will actually meet.
        render("kiosk-pairing", w = 520, h = 900) {
            org.ducatproject.ducat.ui.PairPanel(
                order = order,
                cardUri = "ducat:card/v1?k=" + "a".repeat(52) + "&n=Corner%20Caf%C3%A9",
                error = null,
                onCancel = {},
                onFallback = {},
            )
        }
        // The card could not be published — no node, a dead radio. The person
        // standing there still has to be told something.
        render("kiosk-pairing-failed", w = 520, h = 900) {
            org.ducatproject.ducat.ui.PairPanel(
                order = order,
                cardUri = null,
                error = "the node is not attached",
                onCancel = {},
                onFallback = {},
            )
        }
        // Billed into their conversation; the rest happens on their phone.
        render("kiosk-billed", w = 520, h = 900) {
            org.ducatproject.ducat.ui.BilledPanel(
                order = order.copy(tabId = "t-1", personaHex = "ab".repeat(16)),
                onDone = {},
            )
        }
    }
    // The gate's other mood. The one above is a device that has never had a
    // PIN, which is offered the chance to set one; this is every time after,
    // which is the one somebody meets while holding a customer.
    org.ducatproject.ducat.Pin.set(context, "1234")
    render("pin-ask", w = 700, h = 700) {
        org.ducatproject.ducat.ui.PinGate(open = true, onDismiss = {}, onPassed = {})
    }
    // And the same gate on a phone that has a lock of its own to offer. The
    // desk has no backend — that is the whole reason DeviceLock is a hook —
    // so stand one up that says yes to `enrolled` and never prompts, which is
    // exactly the state the button is drawn in.
    org.ducatproject.ducat.DeviceLock.backend =
        object : org.ducatproject.ducat.DeviceLock.Backend {
            override fun enrolled(context: android.content.Context) = true
            override fun prompt(
                context: android.content.Context,
                title: String,
                subtitle: String,
                onResult: (Boolean) -> Unit,
            ) = Unit
        }
    render("pin-ask-device", w = 700, h = 700) {
        org.ducatproject.ducat.ui.PinGate(open = true, onDismiss = {}, onPassed = {})
    }
    org.ducatproject.ducat.DeviceLock.backend = null

    // --- right to left ----------------------------------------------------
    //
    // Arabic and Persian have shipped for a while and no machine has ever
    // drawn a screen in either. Two things go wrong here that never show up in
    // English: a layout that reads `start` as `left` mirrors wrongly or not at
    // all, and a translation longer than its English source overruns a row
    // that had exactly enough space for the original.
    //
    // The locale and the layout direction are separate switches and both have
    // to be thrown — words in Arabic inside a left-to-right frame is the bug,
    // not the test.
    android.res.DeskRes.setLocale("ar")
    fun rtl(name: String, w: Int, h: Int, content: @Composable () -> Unit) =
        render(name, w, h) {
            CompositionLocalProvider(
                androidx.compose.ui.platform.LocalLayoutDirection provides
                    androidx.compose.ui.unit.LayoutDirection.Rtl,
            ) { content() }
        }

    rtl("ar-kiosk", 520, 900) { org.ducatproject.ducat.ui.KioskScreen() }
    rtl("ar-items", 900, 700) { org.ducatproject.ducat.ui.ItemsScreen() }
    rtl("ar-till", 1100, 800) { org.ducatproject.ducat.ui.PosScreen() }
    rtl("ar-pin", 700, 700) {
        org.ducatproject.ducat.ui.PinGate(open = true, onDismiss = {}, onPassed = {})
    }
    rtl("ar-activity", 1100, 800) { org.ducatproject.ducat.ui.ActivityScreen() }
    android.res.DeskRes.setLocale("en")

    // --- a small phone ----------------------------------------------------
    //
    // 320 by 640 is the floor Android still ships — a Go device, an old
    // handset, the phone somebody running a market stall actually owns rather
    // than the one this was written on. Rows that fit at 520 wrap or clip
    // here, and a till whose total is cut off is a till nobody can use.
    render("small-till", 320, 640) { org.ducatproject.ducat.ui.PosScreen() }
    render("small-kiosk", 320, 640) { org.ducatproject.ducat.ui.KioskScreen() }
    render("small-items", 320, 640) { org.ducatproject.ducat.ui.ItemsScreen() }
    render("small-pin", 320, 640) {
        org.ducatproject.ducat.ui.PinGate(open = true, onDismiss = {}, onPassed = {})
    }

    println(
        if (failures == 0) "RENDERTEST OK — ${out.absolutePath}"
        else "RENDERTEST FAILED ($failures)",
    )
    if (failures > 0) kotlin.system.exitProcess(1)
}
