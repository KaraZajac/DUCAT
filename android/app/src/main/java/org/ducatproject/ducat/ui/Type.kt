package org.ducatproject.ducat.ui

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * The type scale, which is most of what "looks like a real payment app" means.
 *
 * PayPal, Venmo and Uber differ in palette and agree on structure: **one huge
 * number per screen and everything else quiet**. The scale here encodes that —
 * displays are heavy with tight letter-spacing (large text naturally spreads;
 * pulling it back in is what makes a figure look set rather than typed), labels
 * are small and calm, and nothing between them competes.
 *
 * System Roboto throughout, deliberately. A bundled brand font is how those
 * apps get their last few percent, but weight and spacing carry most of it,
 * and a font shipped in the APK is 300KB of somebody else's licence terms.
 */
val DucatTypography = Typography(
    // The hero: a balance, a total, an amount about to be sent.
    displayLarge = TextStyle(
        fontSize = 48.sp, lineHeight = 54.sp,
        fontWeight = FontWeight.Bold, letterSpacing = (-1.2).sp,
    ),
    displayMedium = TextStyle(
        fontSize = 38.sp, lineHeight = 44.sp,
        fontWeight = FontWeight.Bold, letterSpacing = (-0.8).sp,
    ),
    displaySmall = TextStyle(
        fontSize = 30.sp, lineHeight = 36.sp,
        fontWeight = FontWeight.Bold, letterSpacing = (-0.4).sp,
    ),
    headlineLarge = TextStyle(
        fontSize = 28.sp, lineHeight = 34.sp,
        fontWeight = FontWeight.Bold, letterSpacing = (-0.3).sp,
    ),
    headlineMedium = TextStyle(
        fontSize = 24.sp, lineHeight = 30.sp,
        fontWeight = FontWeight.Bold, letterSpacing = (-0.2).sp,
    ),
    headlineSmall = TextStyle(
        fontSize = 21.sp, lineHeight = 27.sp,
        fontWeight = FontWeight.SemiBold, letterSpacing = (-0.1).sp,
    ),
    titleLarge = TextStyle(
        fontSize = 19.sp, lineHeight = 25.sp, fontWeight = FontWeight.SemiBold,
    ),
    titleMedium = TextStyle(
        fontSize = 16.sp, lineHeight = 22.sp,
        fontWeight = FontWeight.SemiBold, letterSpacing = 0.1.sp,
    ),
    titleSmall = TextStyle(
        fontSize = 14.sp, lineHeight = 20.sp,
        fontWeight = FontWeight.SemiBold, letterSpacing = 0.1.sp,
    ),
    bodyLarge = TextStyle(
        fontSize = 16.sp, lineHeight = 24.sp, letterSpacing = 0.3.sp,
    ),
    bodyMedium = TextStyle(
        fontSize = 14.sp, lineHeight = 21.sp, letterSpacing = 0.2.sp,
    ),
    bodySmall = TextStyle(
        fontSize = 12.sp, lineHeight = 17.sp, letterSpacing = 0.2.sp,
    ),
    // Labels: what buttons and chips wear.
    labelLarge = TextStyle(
        fontSize = 15.sp, lineHeight = 20.sp,
        fontWeight = FontWeight.SemiBold, letterSpacing = 0.2.sp,
    ),
    labelMedium = TextStyle(
        fontSize = 12.sp, lineHeight = 16.sp,
        fontWeight = FontWeight.Medium, letterSpacing = 0.4.sp,
    ),
    labelSmall = TextStyle(
        fontSize = 11.sp, lineHeight = 15.sp,
        fontWeight = FontWeight.Medium, letterSpacing = 0.4.sp,
    ),
)
