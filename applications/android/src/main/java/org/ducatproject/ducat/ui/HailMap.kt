package org.ducatproject.ducat.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.viewinterop.AndroidView
import org.ducatproject.ducat.R
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.BoundingBox
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker
import org.osmdroid.views.overlay.Polyline

/**
 * The ride, drawn. OSM tiles via osmdroid — which means tile requests go to
 * OpenStreetMap's servers, a location leak the hail sheet states in words.
 * Display only: nothing here feeds the protocol, which sees cells and text.
 */
@Composable
fun RouteMap(
    from: Pair<Long, Long>?,
    to: Pair<Long, Long>?,
    route: List<Pair<Long, Long>>,
    modifier: Modifier = Modifier,
) {
    AndroidView(
        modifier = modifier.clipToBounds(),
        factory = { ctx ->
            // The full init, not just a UA: without load(), osmdroid has no
            // cache path and quietly renders the grey grid of nothing.
            val cfg = org.osmdroid.config.Configuration.getInstance()
            cfg.load(ctx, ctx.getSharedPreferences("osmdroid", 0))
            cfg.userAgentValue = "DUCAT/0.8 (github.com/KaraZajac/DUCAT)"
            cfg.osmdroidBasePath = java.io.File(ctx.cacheDir, "osm")
            cfg.osmdroidTileCache = java.io.File(ctx.cacheDir, "osm/tiles")
            MapView(ctx).apply {
                setTileSource(TileSourceFactory.MAPNIK)
                setMultiTouchControls(true)
                clipToOutline = true
                // One Earth is enough: repetition is what a failed zoom shows
                // three of, vertically.
                isVerticalMapRepetitionEnabled = false
                isHorizontalMapRepetitionEnabled = false
                minZoomLevel = 3.0
                controller.setZoom(14.0)
            }
        },
        update = { map ->
            map.overlays.clear()
            val pts = ArrayList<GeoPoint>()
            from?.let {
                val g = GeoPoint(it.first / 1e7, it.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = map.context.getString(R.string.hailmap_pickup)
                    icon = hereDot(map.context)
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
                })
            }
            to?.let {
                val g = GeoPoint(it.first / 1e7, it.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = map.context.getString(R.string.hailmap_destination)
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                })
            }
            if (route.isNotEmpty()) {
                map.overlays.add(Polyline(map).apply {
                    setPoints(route.map { GeoPoint(it.first / 1e7, it.second / 1e7) })
                    outlinePaint.strokeWidth = 8f
                })
            }
            // Deferred: zoomToBoundingBox on a view that has not been laid
            // out yet has zero pixels to fit the box into, and osmdroid's
            // answer to that is the whole world — observed as a 37 km ride
            // rendered as three planets. post{} runs after measurement.
            map.post {
                when {
                    pts.size >= 2 -> map.zoomToBoundingBox(
                        BoundingBox.fromGeoPoints(pts).increaseByScale(1.4f), false,
                    )
                    pts.size == 1 -> {
                        map.controller.setZoom(14.0)
                        map.controller.setCenter(pts[0])
                    }
                }
                map.invalidate()
            }
        },
    )
}

/**
 * Search results as pins, before one of them has been chosen.
 *
 * A list of ten branches of the same chain is ten near-identical lines, and
 * the distance beside each answers "how far" without answering "which way".
 * Seeing them laid out is how a person picks the one on their side of the
 * river rather than the one that happens to be four hundred metres nearer as
 * the crow flies.
 *
 * **Only when there is something to compare.** The caller draws this at two
 * results or more: one result has nothing to be laid out against, and tiles
 * come from OpenStreetMap's servers — already the one place this app sends
 * location off-device, and stated as such on the sheet, but no reason to ask
 * for them when the map would say nothing.
 *
 * Tapping a pin picks it, exactly as tapping its row does.
 */
@Composable
fun ResultsMap(
    me: Pair<Long, Long>?,
    results: List<Pair<Pair<Long, Long>, String>>,
    onPick: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    AndroidView(
        modifier = modifier.clipToBounds(),
        factory = { ctx ->
            val cfg = org.osmdroid.config.Configuration.getInstance()
            cfg.load(ctx, ctx.getSharedPreferences("osmdroid", 0))
            cfg.userAgentValue = "DUCAT/0.8 (github.com/KaraZajac/DUCAT)"
            cfg.osmdroidBasePath = java.io.File(ctx.cacheDir, "osm")
            cfg.osmdroidTileCache = java.io.File(ctx.cacheDir, "osm/tiles")
            MapView(ctx).apply {
                setTileSource(TileSourceFactory.MAPNIK)
                setMultiTouchControls(true)
                clipToOutline = true
                isVerticalMapRepetitionEnabled = false
                isHorizontalMapRepetitionEnabled = false
                minZoomLevel = 3.0
                controller.setZoom(13.0)
            }
        },
        update = { map ->
            map.overlays.clear()
            val pts = ArrayList<GeoPoint>()
            me?.let {
                val g = GeoPoint(it.first / 1e7, it.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = map.context.getString(R.string.hailmap_you)
                    icon = hereDot(map.context)
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
                })
            }
            results.forEachIndexed { i, (at, label) ->
                val g = GeoPoint(at.first / 1e7, at.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = label
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                    setOnMarkerClickListener { _, _ -> onPick(i); true }
                })
            }
            // Same deferral as RouteMap, for the same reason: fitting a box
            // into a view with no pixels yet gets you the whole planet.
            map.post {
                when {
                    pts.size >= 2 -> map.zoomToBoundingBox(
                        BoundingBox.fromGeoPoints(pts).increaseByScale(1.5f), false,
                    )
                    pts.size == 1 -> {
                        map.controller.setZoom(14.0)
                        map.controller.setCenter(pts[0])
                    }
                }
                map.invalidate()
            }
        },
    )
}

/**
 * The driver's watch: demand as pins. One marker per standing hail at its
 * pickup cell's centre (~1.2 km coarse — the board's honesty carries to the
 * map), the driver's own position beside them. Tap a pin to read the job.
 */
@Composable
fun DriverMap(
    me: Pair<Long, Long>?,
    fares: List<Pair<Pair<Long, Long>, String>>,
    onFareTap: (Int) -> Unit,
    /** The watched area's outer bounds (latLo, latHi, lonLo, lonHi, e7) —
     *  the driver's net, drawn instead of guessed. */
    coverage: LongArray? = null,
    modifier: Modifier = Modifier,
) {
    AndroidView(
        modifier = modifier.clipToBounds(),
        factory = { ctx ->
            val cfg = org.osmdroid.config.Configuration.getInstance()
            cfg.load(ctx, ctx.getSharedPreferences("osmdroid", 0))
            cfg.userAgentValue = "DUCAT/0.8 (github.com/KaraZajac/DUCAT)"
            cfg.osmdroidBasePath = java.io.File(ctx.cacheDir, "osm")
            cfg.osmdroidTileCache = java.io.File(ctx.cacheDir, "osm/tiles")
            MapView(ctx).apply {
                setTileSource(TileSourceFactory.MAPNIK)
                setMultiTouchControls(true)
                clipToOutline = true
                isVerticalMapRepetitionEnabled = false
                isHorizontalMapRepetitionEnabled = false
                minZoomLevel = 3.0
                controller.setZoom(13.0)
            }
        },
        update = { map ->
            map.overlays.clear()
            val pts = ArrayList<GeoPoint>()
            coverage?.let { c ->
                val (latLo, latHi, lonLo, lonHi) = listOf(c[0], c[1], c[2], c[3])
                val box = listOf(
                    GeoPoint(latLo / 1e7, lonLo / 1e7),
                    GeoPoint(latLo / 1e7, lonHi / 1e7),
                    GeoPoint(latHi / 1e7, lonHi / 1e7),
                    GeoPoint(latHi / 1e7, lonLo / 1e7),
                )
                map.overlays.add(org.osmdroid.views.overlay.Polygon(map).apply {
                    points = box + box.first()
                    outlinePaint.strokeWidth = 4f
                    outlinePaint.color = android.graphics.Color.argb(200, 150, 120, 255)
                    fillPaint.color = android.graphics.Color.argb(24, 150, 120, 255)
                })
                // The frame includes the whole net, not just where things are.
                pts += box
            }
            me?.let {
                val g = GeoPoint(it.first / 1e7, it.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = map.context.getString(R.string.hailmap_you)
                    icon = hereDot(map.context)
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
                })
            }
            fares.forEachIndexed { i, (at, label) ->
                val g = GeoPoint(at.first / 1e7, at.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = label
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                    setOnMarkerClickListener { _, _ -> onFareTap(i); true }
                })
            }
            map.post {
                when {
                    pts.size >= 2 -> map.zoomToBoundingBox(
                        BoundingBox.fromGeoPoints(pts).increaseByScale(1.5f), false,
                    )
                    pts.size == 1 -> {
                        map.controller.setZoom(13.0)
                        map.controller.setCenter(pts[0])
                    }
                }
                map.invalidate()
            }
        },
    )
}

/**
 * "You are here", as a dot rather than another pin.
 *
 * Every marker on both of these maps was osmdroid's default teardrop, so a
 * rider's pickup looked exactly like their destination and a driver's own
 * position looked exactly like a fare. The titles differ, but a title costs a
 * tap, and the question a map has to answer without one is "which of these is
 * me". A dot for where you are and a pin for where you are going is the idiom
 * every other map already taught people.
 *
 * Drawn rather than shipped: it is two circles, and a drawable in the
 * resources would need its own copy per density and a name in nineteen
 * languages' worth of nothing.
 */
private fun hereDot(context: android.content.Context): android.graphics.drawable.Drawable {
    val d = context.resources.displayMetrics.density
    val r = 7f * d
    val ring = 2.5f * d
    val size = ((r + ring) * 2f).toInt().coerceAtLeast(1)
    val bmp = android.graphics.Bitmap.createBitmap(
        size, size, android.graphics.Bitmap.Config.ARGB_8888,
    )
    val canvas = android.graphics.Canvas(bmp)
    val paint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG)
    val mid = size / 2f
    // White first, so the dot reads against a dark tile as well as a pale one.
    paint.color = android.graphics.Color.WHITE
    canvas.drawCircle(mid, mid, r + ring, paint)
    paint.color = android.graphics.Color.rgb(0x1A, 0x73, 0xE8)
    canvas.drawCircle(mid, mid, r, paint)
    return android.graphics.drawable.BitmapDrawable(context.resources, bmp)
}
