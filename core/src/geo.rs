//! Geocells (§15.12): turning a place on Earth into a stand name.
//!
//! A geohash interleaves latitude and longitude bisections into a base32
//! string with one property this protocol leans on entirely: **every prefix
//! is a containing cell**. `9q8yy` contains `9q8yyk`; a cell name is a place
//! at a stated coarseness, and coarseness is the privacy dial. Cell sizes by
//! length: 4 ≈ 39×20 km, 5 ≈ 4.9×4.9 km, 6 ≈ 1.2×0.6 km — and §16.17 caps
//! what may appear on a public board at 6, so "no precise location" is true
//! by construction rather than by good manners.
//!
//! **Integer arithmetic throughout.** Coordinates arrive as 1e-7 degrees
//! (the resolution GPS hardware reports), ranges bisect with floor midpoints,
//! and no float touches the path — because two implementations disagreeing on
//! a cell boundary is two people standing on the same corner posting to
//! different boards, which is exactly the failure class the ring vectors
//! exist to prevent. Precision is capped at 9 (≈ 4.8 m) where integer and
//! textbook float encodings still agree everywhere except a boundary thinner
//! than GPS noise.

use crate::reject::{Reject, RejectCode};

/// The geohash alphabet, standard since the original implementation.
/// Note the absent letters: a, i, l, o.
const ALPHABET: &[u8; 32] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// Longest cell name this module will mint or parse.
pub const MAX_GEOHASH_CHARS: usize = 9;
/// Longest cell that may appear on a public surface (§16.17).
pub const MAX_BOARD_GEOHASH_CHARS: usize = 6;

const LAT_MAX_E7: i64 = 900_000_000;
const LON_MAX_E7: i64 = 1_800_000_000;

fn bad(detail: &'static str) -> Reject {
    Reject::with_detail(RejectCode::Malformed, detail)
}

/// Encode a coordinate to a geohash of `precision` characters.
///
/// Truncation, never rounding: the cell containing the point, which is the
/// only answer that cannot jump a boundary (§15.12).
pub fn geohash_encode(lat_e7: i64, lon_e7: i64, precision: u32) -> Result<String, Reject> {
    if !(1..=MAX_GEOHASH_CHARS as u32).contains(&precision) {
        return Err(bad("geohash precision is 1..=9"));
    }
    if !(-LAT_MAX_E7..=LAT_MAX_E7).contains(&lat_e7) {
        return Err(bad("latitude out of range"));
    }
    if !(-LON_MAX_E7..=LON_MAX_E7).contains(&lon_e7) {
        return Err(bad("longitude out of range"));
    }

    let (mut lat_lo, mut lat_hi) = (-LAT_MAX_E7, LAT_MAX_E7);
    let (mut lon_lo, mut lon_hi) = (-LON_MAX_E7, LON_MAX_E7);
    let mut out = String::with_capacity(precision as usize);
    let mut bits = 0u32;
    let mut ch = 0u8;
    let mut even = true; // longitude first, by convention

    while out.len() < precision as usize {
        if even {
            let mid = midpoint(lon_lo, lon_hi);
            if lon_e7 >= mid {
                ch = (ch << 1) | 1;
                lon_lo = mid;
            } else {
                ch <<= 1;
                lon_hi = mid;
            }
        } else {
            let mid = midpoint(lat_lo, lat_hi);
            if lat_e7 >= mid {
                ch = (ch << 1) | 1;
                lat_lo = mid;
            } else {
                ch <<= 1;
                lat_hi = mid;
            }
        }
        even = !even;
        bits += 1;
        if bits == 5 {
            out.push(ALPHABET[ch as usize] as char);
            bits = 0;
            ch = 0;
        }
    }
    Ok(out)
}

/// Floor midpoint, negative-safe: `>>` floors where `/` truncates toward
/// zero, and an encoder and decoder flooring differently is a boundary
/// dispute.
fn midpoint(lo: i64, hi: i64) -> i64 {
    (lo + hi) >> 1
}

/// The cell's bounds: (lat_lo, lat_hi, lon_lo, lon_hi), all e7.
fn bounds(cell: &str) -> Result<(i64, i64, i64, i64), Reject> {
    if cell.is_empty() || cell.len() > MAX_GEOHASH_CHARS {
        return Err(bad("a geohash is 1..=9 characters"));
    }
    let (mut lat_lo, mut lat_hi) = (-LAT_MAX_E7, LAT_MAX_E7);
    let (mut lon_lo, mut lon_hi) = (-LON_MAX_E7, LON_MAX_E7);
    let mut even = true;
    for c in cell.bytes() {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c.to_ascii_lowercase())
            .ok_or_else(|| bad("not a geohash character"))? as u8;
        for shift in (0..5).rev() {
            let bit = (v >> shift) & 1;
            if even {
                let mid = midpoint(lon_lo, lon_hi);
                if bit == 1 { lon_lo = mid } else { lon_hi = mid }
            } else {
                let mid = midpoint(lat_lo, lat_hi);
                if bit == 1 { lat_lo = mid } else { lat_hi = mid }
            }
            even = !even;
        }
    }
    Ok((lat_lo, lat_hi, lon_lo, lon_hi))
}

/// The cell's bounds, e7: (lat_lo, lat_hi, lon_lo, lon_hi). What lets a map
/// draw the area a name covers — a driver's net, visible instead of guessed.
pub fn geohash_bounds(cell: &str) -> Result<(i64, i64, i64, i64), Reject> {
    bounds(cell)
}

/// The cell's centre, e7. What "distance to this cell" means.
pub fn geohash_center(cell: &str) -> Result<(i64, i64), Reject> {
    let (lat_lo, lat_hi, lon_lo, lon_hi) = bounds(cell)?;
    Ok((midpoint(lat_lo, lat_hi), midpoint(lon_lo, lon_hi)))
}

/// The 8 surrounding cells, clockwise from north:
/// N, NE, E, SE, S, SW, W, NW.
///
/// This is the driver's watch set (§15.12): a rider fifty metres over a cell
/// border is invisible to a driver watching one board, so a driver watches
/// nine. Longitude wraps at the antimeridian; latitude clamps at the poles,
/// where a "neighbour" repeats the cell itself — nobody hails a cab at 90°N,
/// and an error there would be less honest than the repetition.
pub fn geohash_neighbors(cell: &str) -> Result<Vec<String>, Reject> {
    let (lat_lo, lat_hi, lon_lo, lon_hi) = bounds(cell)?;
    let clat = midpoint(lat_lo, lat_hi);
    let clon = midpoint(lon_lo, lon_hi);
    let h = lat_hi - lat_lo;
    let w = lon_hi - lon_lo;
    let precision = cell.len() as u32;

    let deltas: [(i64, i64); 8] = [
        (h, 0),      // N
        (h, w),      // NE
        (0, w),      // E
        (-h, w),     // SE
        (-h, 0),     // S
        (-h, -w),    // SW
        (0, -w),     // W
        (h, -w),     // NW
    ];
    let mut out = Vec::with_capacity(8);
    for (dlat, dlon) in deltas {
        let lat = (clat + dlat).clamp(-LAT_MAX_E7, LAT_MAX_E7);
        let mut lon = clon + dlon;
        if lon > LON_MAX_E7 {
            lon -= 2 * LON_MAX_E7;
        } else if lon < -LON_MAX_E7 {
            lon += 2 * LON_MAX_E7;
        }
        out.push(geohash_encode(lat, lon, precision)?);
    }
    Ok(out)
}

/// Great-circle distance between two points, in metres.
///
/// Haversine on a sphere — floats are fine *here*, because a distance is an
/// estimate shown to a person, never a value two implementations must agree
/// on byte-for-byte. The road always adds its own factor on top anyway
/// (§15.12's fare estimate multiplies by circuity).
pub fn haversine_m(lat1_e7: i64, lon1_e7: i64, lat2_e7: i64, lon2_e7: i64) -> u64 {
    const R: f64 = 6_371_000.0;
    let to_rad = |e7: i64| (e7 as f64 / 1e7).to_radians();
    let (la1, lo1, la2, lo2) = (to_rad(lat1_e7), to_rad(lon1_e7), to_rad(lat2_e7), to_rad(lon2_e7));
    let dlat = la2 - la1;
    let dlon = lo2 - lo1;
    let a = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    (2.0 * R * a.sqrt().asin()) as u64
}

/// A board cell is a valid geohash no finer than precision 6 (§16.17).
pub fn valid_board_cell(cell: &str) -> bool {
    !cell.is_empty()
        && cell.len() <= MAX_BOARD_GEOHASH_CHARS
        && cell.bytes().all(|c| ALPHABET.contains(&c.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classics, from the original geohash description — the external
    /// validation this module gets before the vectors freeze its own answers.
    #[test]
    fn encodes_the_known_answers() {
        // 42.605, -5.603 → ezs42 (the worked example everyone reproduces)
        assert_eq!(geohash_encode(426_050_000, -56_030_000, 5).unwrap(), "ezs42");
        // Jutland's classic: 57.64911, 10.40744 → u4pruydqqvj (truncated)
        assert_eq!(geohash_encode(576_491_100, 104_074_400, 9).unwrap(), "u4pruydqq");
        // Prefix property: coarser is a prefix of finer.
        let fine = geohash_encode(426_050_000, -56_030_000, 9).unwrap();
        assert!(fine.starts_with("ezs42"));
    }

    #[test]
    fn center_round_trips() {
        let c = geohash_encode(387_747_300, -771_942_700, 6).unwrap();
        let (lat, lon) = geohash_center(&c).unwrap();
        assert_eq!(geohash_encode(lat, lon, 6).unwrap(), c);
    }

    #[test]
    fn neighbors_are_mutual_and_distinct() {
        let cell = geohash_encode(387_747_300, -771_942_700, 6).unwrap();
        let n = geohash_neighbors(&cell).unwrap();
        assert_eq!(n.len(), 8);
        let mut all = n.clone();
        all.push(cell.clone());
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 9, "all nine cells distinct away from poles");
        // North's south is home again.
        let north = &n[0];
        let back = geohash_neighbors(north).unwrap();
        assert_eq!(back[4], cell);
    }

    #[test]
    fn refuses_junk() {
        assert!(geohash_encode(0, 0, 0).is_err());
        assert!(geohash_encode(0, 0, 10).is_err());
        assert!(geohash_encode(LAT_MAX_E7 + 1, 0, 5).is_err());
        assert!(geohash_neighbors("ezs4a").is_err()); // 'a' is not in the alphabet
        assert!(!valid_board_cell("u4pruyd")); // 7 chars: too precise for a board
        assert!(valid_board_cell("u4pruy"));
    }

    #[test]
    fn haversine_sane() {
        // DCA to IAD is ~37 km as the crow flies.
        let d = haversine_m(388_522_000, -770_377_000, 389_531_000, -774_565_000);
        assert!((30_000..45_000).contains(&d), "got {d}");
        assert_eq!(haversine_m(1, 1, 1, 1), 0);
    }
}
