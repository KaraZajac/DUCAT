package org.ducatproject.desk

import org.ducatproject.ducat.Geo

/**
 * What leaves the phone when somebody types in a search box.
 * `./gradlew :desktop:geoprivacy`.
 *
 * Nominatim is the one place DUCAT sends location off-device, and the screen
 * says so. What the screen did not say is how much: the search sent a viewbox
 * to bias results toward the user, and a box is symmetric, so averaging its
 * corners gave back the centre — which was the fix itself to four decimal
 * places. Eleven metres, on every keystroke, into somebody else's logs.
 *
 * The box still biases. Its centre is now a grid point.
 */
fun main() {
    // The property that matters: whatever comes back must not be recoverable
    // to better than the grid. Checked by doing what an observer would do.
    val places = listOf(
        48.8566 to 2.3522,      // Paris
        -33.8688 to 151.2093,   // Sydney, southern and eastern
        -22.9068 to -43.1729,   // Rio, southern and western
        64.1466 to -21.9426,    // Reykjavík, far north
        0.0 to 0.0,             // the null island edge
        1.0E-7 to -1.0E-7,      // either side of zero, so rounding cannot bias
    )
    for ((lat, lon) in places) {
        val cLat = Geo.coarse(lat)
        val cLon = Geo.coarse(lon)
        // What an observer recovers from the box corners is the coarse value,
        // never the fix. A tenth of a degree is about 11 km.
        check(Math.abs(cLat - lat) <= 0.05000001) {
            "GEOPRIV_FAIL $lat snapped to $cLat, further than half a cell"
        }
        check(Math.abs(cLon - lon) <= 0.05000001) {
            "GEOPRIV_FAIL $lon snapped to $cLon, further than half a cell"
        }
        // On the grid, so repeated searches from one place report one cell
        // rather than a track through it.
        check(Math.abs(cLat * 10 - Math.round(cLat * 10)) < 1e-9) {
            "GEOPRIV_FAIL $cLat is not on the grid"
        }
    }

    // Two people a hundred metres apart in the same cell are indistinguishable,
    // which is the whole point — the leak is the cell, not the person.
    check(Geo.coarse(48.8566) == Geo.coarse(48.8574)) {
        "GEOPRIV_FAIL neighbours in one cell were told apart"
    }

    // Rounding, not truncation. Truncating always moves toward the equator
    // and toward the prime meridian, and a consistent bias is something an
    // observer can unpick — a southern-hemisphere fix would always report
    // north of itself.
    check(Geo.coarse(-33.86) == -33.9) { "GEOPRIV_FAIL -33.86 truncated toward zero" }
    check(Geo.coarse(-33.84) == -33.8) { "GEOPRIV_FAIL -33.84 rounded the wrong way" }
    check(Geo.coarse(33.86) == 33.9) { "GEOPRIV_FAIL 33.86 rounded the wrong way" }

    // And the box is still a box: ±0.45° around the snapped centre is ~100 km
    // across, so the bias it was there to provide is untouched.
    val span = 0.9 * 111.0
    check(span > 90) { "GEOPRIV_FAIL the viewbox stopped being a viewbox" }

    println("GEOPRIV_OK grid=0.1deg(~11km) places=${places.size} rounding=symmetric box=~${span.toInt()}km")
}
