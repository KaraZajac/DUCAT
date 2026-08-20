package org.ducatproject.ducat

/**
 * What a ride costs where you are.
 *
 * §15.12's fare suggestion used four constants — a $2.00 base, $0.65/km,
 * $0.25/min, a $6.00 minimum — and then divided the result by whatever
 * exchange rate the user's *chosen currency* happened to have. So the dollars
 * were silently reread as rupees, yen or dinars: an eight-kilometre ride
 * offered ₹11.20 in India, about thirteen US cents, and KWD 11.20 in Kuwait,
 * about thirty-six dollars. The number was right in one country and wrong in
 * every other, and nothing said so.
 *
 * ## The model
 *
 * Two measured numbers per country — what a taxi charges to start, and what
 * it charges per kilometre — and everything else derived from them by ratios
 * taken from the market the old constants described. Those ratios are not
 * invented: anchoring on the United States' own taxi figures (a $3.50 start
 * and $1.70/km) reproduces the previous Uber and DUCAT constants exactly,
 * which is the check that the shape is right.
 *
 *   per-minute      = 0.0588 × per-km      (US: 1.70 → 0.10)
 *   rideshare fixed = 1.357 × taxi start   (US: 3.50 → 4.75, base + booking)
 *   rideshare /km   = 0.441 × taxi /km     (US: 1.70 → 0.75)
 *   rideshare /min  = 3.0   × taxi /min    (US: 0.10 → 0.30)
 *   rideshare min   = 2.143 × taxi start   (US: 3.50 → 7.50)
 *
 *   DUCAT base      = 0.571 × taxi start   (US: 3.50 → 2.00)
 *   DUCAT /km       = 0.382 × taxi /km     (US: 1.70 → 0.65)
 *   DUCAT /min      = 2.5   × taxi /min    (US: 0.10 → 0.25)
 *   DUCAT minimum   = 1.714 × taxi start   (US: 3.50 → 6.00)
 *
 * DUCAT lands under the rideshare's rider price while paying the driver more
 * than the rideshare's driver payout, which is the whole argument of §15.12
 * and works in any currency because the ratio, not the amount, is what
 * carries it. A local rideshare is not always 0.44 of a local taxi; it is the
 * best available estimate where nobody publishes the real number, and every
 * figure here is a *suggestion a driver can type over* rather than a price.
 *
 * ## The units
 *
 * Everything in this file is **US dollars**, because that is the currency the
 * survey normalises to, and a table in a hundred currencies could not be
 * compared or checked. Converting to what a rider actually sees happens once,
 * against a USD/XMR rate fetched exactly like their own currency's — see
 * [Fare]. Keeping one unit in the table is what stops the old bug returning:
 * there is no longer a number here whose currency depends on a setting.
 *
 * ## The data
 *
 * Taxi start and per-kilometre by country, Numbeo's crowd-sourced survey,
 * read 2026-08-20. It is the only source that covers a hundred countries on
 * one comparable basis. It is a survey: it is approximate, it lags, and in
 * places where a metered fare is a fiction it records what people actually
 * pay, which is arguably the more useful number for this.
 *
 * A country nobody has surveyed falls back to [DEFAULT], the median of the
 * table rather than the United States — the median country is a great deal
 * poorer than the US, and guessing high would have a driver quote a fare
 * nobody local would pay.
 */
object FareRates {

    /** A taxi's start and per-kilometre charge, in US dollars. */
    data class Local(val start: Double, val perKm: Double)

    /** per-minute from per-km; the US pair is 1.70 and 0.10. */
    const val MIN_FROM_KM = 0.0588

    // Rideshare, against the local taxi.
    const val RIDESHARE_FIXED = 1.357
    const val RIDESHARE_PER_KM = 0.441
    const val RIDESHARE_PER_MIN = 3.0
    const val RIDESHARE_MIN = 2.143

    /** What a rideshare hands its driver: the platform's ~29% take, absent here. */
    const val DRIVER_SHARE = 0.71

    // DUCAT's own suggestion, against the local taxi.
    const val OURS_BASE = 0.571
    const val OURS_PER_KM = 0.382
    const val OURS_PER_MIN = 2.5
    const val OURS_MIN = 1.714

    /**
     * The median of the table, for a country the survey does not cover.
     *
     * Median rather than mean or United States: the distribution has a long
     * rich tail — Switzerland charges eighteen times what the Philippines
     * does — so a mean would sit well above where most people live, and
     * quoting a Swiss fare in an uncovered country is worse than quoting a
     * middling one.
     */
    val DEFAULT = Local(start = 2.28, perKm = 1.09)

    /**
     * Taxi start and per-km in USD, by ISO 3166-1 alpha-2.
     *
     * Numbeo, 2026-08-20. Sorted by code so a future update can be diffed
     * against the source rather than read.
     */
    private val TABLE: Map<String, Local> = mapOf(
        "AE" to Local(3.27, 0.68), "AL" to Local(3.77, 3.77), "AM" to Local(1.10, 0.42),
        "AR" to Local(1.60, 1.00), "AT" to Local(5.66, 2.34), "AU" to Local(3.56, 1.63),
        "AZ" to Local(1.76, 0.59), "BA" to Local(1.48, 1.19), "BD" to Local(0.82, 0.41),
        "BE" to Local(5.84, 2.80), "BG" to Local(1.55, 0.78), "BH" to Local(5.30, 3.45),
        "BO" to Local(1.30, 0.87), "BR" to Local(1.26, 0.97), "BY" to Local(0.99, 0.55),
        "CA" to Local(3.20, 1.52), "CH" to Local(8.21, 4.76), "CL" to Local(0.87, 1.19),
        "CN" to Local(1.49, 0.36), "CO" to Local(2.27, 2.27), "CR" to Local(2.26, 2.00),
        "CU" to Local(2.14, 0.45), "CY" to Local(7.01, 2.34), "CZ" to Local(2.42, 1.62),
        "DE" to Local(5.60, 2.92), "DK" to Local(7.65, 2.03), "DO" to Local(3.40, 2.30),
        "DZ" to Local(1.17, 0.47), "EC" to Local(1.50, 1.50), "EE" to Local(3.74, 1.17),
        "EG" to Local(0.39, 0.30), "ES" to Local(4.66, 1.52), "FI" to Local(9.34, 1.40),
        "FR" to Local(5.14, 2.22), "GB" to Local(5.44, 2.20), "GE" to Local(1.15, 0.69),
        "GR" to Local(4.67, 1.17), "HK" to Local(3.70, 1.34), "HR" to Local(3.88, 1.52),
        "HU" to Local(3.53, 1.43), "ID" to Local(0.56, 0.37), "IE" to Local(5.55, 1.69),
        "IL" to Local(4.43, 1.31), "IN" to Local(0.87, 0.26), "IQ" to Local(2.29, 1.53),
        "IR" to Local(0.50, 0.30), "IT" to Local(5.84, 1.75), "JO" to Local(0.59, 1.41),
        "JP" to Local(3.70, 3.16), "KE" to Local(1.54, 1.54), "KG" to Local(0.97, 0.35),
        "KR" to Local(3.31, 0.65), "KW" to Local(3.24, 3.24), "KZ" to Local(1.08, 0.87),
        "LK" to Local(0.45, 0.35), "LT" to Local(3.21, 1.17), "LU" to Local(5.25, 4.15),
        "LV" to Local(3.50, 0.83), "MA" to Local(0.75, 0.75), "MD" to Local(2.33, 0.35),
        "ME" to Local(1.17, 1.17), "MK" to Local(1.33, 0.72), "MT" to Local(5.84, 2.34),
        "MU" to Local(4.26, 3.19), "MX" to Local(2.95, 2.89), "MY" to Local(1.23, 0.99),
        "NG" to Local(0.59, 1.09), "NL" to Local(4.85, 3.27), "NO" to Local(12.22, 1.88),
        "NP" to Local(1.63, 1.04), "NZ" to Local(2.37, 1.99), "OM" to Local(3.25, 1.30),
        "PA" to Local(2.50, 1.84), "PE" to Local(2.38, 2.08), "PH" to Local(0.75, 0.23),
        "PK" to Local(0.90, 0.54), "PL" to Local(2.43, 1.08), "PR" to Local(2.50, 1.50),
        "PT" to Local(4.09, 1.14), "QA" to Local(2.19, 1.92), "RO" to Local(0.83, 0.78),
        "RS" to Local(1.99, 1.00), "RU" to Local(2.38, 0.42), "SA" to Local(2.66, 2.66),
        "SE" to Local(6.47, 2.01), "SG" to Local(3.62, 0.79), "SI" to Local(2.28, 1.40),
        "SK" to Local(3.50, 1.17), "TH" to Local(1.22, 1.20), "TN" to Local(0.31, 0.35),
        "TR" to Local(1.56, 0.90), "TW" to Local(2.82, 0.78), "UA" to Local(2.01, 0.45),
        "US" to Local(3.50, 1.86), "UY" to Local(1.87, 1.62), "UZ" to Local(0.42, 0.34),
        "VE" to Local(5.00, 2.50), "VN" to Local(0.76, 0.57), "XK" to Local(2.34, 0.73),
        "ZA" to Local(1.09, 0.93), "ZW" to Local(3.50, 2.00),
    )

    /** How many countries the survey covers, for the line that says so. */
    val COVERED: Int get() = TABLE.size

    /** True when this country was surveyed rather than defaulted. */
    fun known(country: String): Boolean = TABLE.containsKey(country.uppercase())

    /**
     * The local taxi, or the median if nobody has surveyed here.
     *
     * The country is a plain ISO code and comes from wherever the caller
     * knows best — the device's region, or a driver's own choice. It is
     * deliberately not derived from the *currency*: a euro buys a very
     * different ride in Dublin and in Athens, and the table has both.
     */
    fun of(country: String): Local = TABLE[country.uppercase()] ?: DEFAULT
}
