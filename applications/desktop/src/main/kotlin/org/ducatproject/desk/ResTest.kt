package org.ducatproject.desk

import android.res.DeskRes
import org.ducatproject.ducat.R

/**
 * The resource bridge, checked without a window: ids resolve, languages
 * switch, plurals pick the right class. `./gradlew :desktop:restest`.
 */
fun main() {
    DeskRes.setLocale("en")
    val en = DeskRes.string(R.string.main_back)
    DeskRes.setLocale("es")
    val es = DeskRes.string(R.string.main_back)
    DeskRes.setLocale("ja")
    val ja = DeskRes.string(R.string.main_back)
    println("RESTEST back: en=$en es=$es ja=$ja")

    // Plurals across the families: English one/other, Russian one/few/many.
    DeskRes.setLocale("en")
    println("RESTEST en plural 1=${DeskRes.plural(R.plurals.balance_notes, 1, 1)} " +
        "2=${DeskRes.plural(R.plurals.balance_notes, 2, 2)}")
    DeskRes.setLocale("ru")
    println("RESTEST ru plural 1=${DeskRes.plural(R.plurals.balance_notes, 1, 1)} " +
        "3=${DeskRes.plural(R.plurals.balance_notes, 3, 3)} " +
        "7=${DeskRes.plural(R.plurals.balance_notes, 7, 7)}")
    // Formatting args survive the round trip.
    DeskRes.setLocale("en")
    println("RESTEST format: " + DeskRes.string(R.string.txdetail_copied, "Address"))
    println("RESTEST locales: ${DeskRes.available.size} — ${DeskRes.available.joinToString(",")}")
    check(en != es && es != ja) { "RESTEST_FAIL languages did not differ" }
    println("RESTEST OK")
}
