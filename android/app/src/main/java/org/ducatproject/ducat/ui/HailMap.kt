package org.ducatproject.ducat.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
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
        modifier = modifier,
        factory = { ctx ->
            org.osmdroid.config.Configuration.getInstance().userAgentValue =
                "DUCAT/0.8 (github.com/KaraZajac/DUCAT)"
            MapView(ctx).apply {
                setTileSource(TileSourceFactory.MAPNIK)
                setMultiTouchControls(true)
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
                    position = g; title = "Pickup"
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                })
            }
            to?.let {
                val g = GeoPoint(it.first / 1e7, it.second / 1e7)
                pts += g
                map.overlays.add(Marker(map).apply {
                    position = g; title = "Destination"
                    setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                })
            }
            if (route.isNotEmpty()) {
                map.overlays.add(Polyline(map).apply {
                    setPoints(route.map { GeoPoint(it.first / 1e7, it.second / 1e7) })
                    outlinePaint.strokeWidth = 8f
                })
            }
            when {
                pts.size >= 2 -> map.zoomToBoundingBox(
                    BoundingBox.fromGeoPoints(pts).increaseByScale(1.4f), false,
                )
                pts.size == 1 -> map.controller.setCenter(pts[0])
            }
            map.invalidate()
        },
    )
}
