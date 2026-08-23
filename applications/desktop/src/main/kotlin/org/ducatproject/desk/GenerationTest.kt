package org.ducatproject.desk

import org.ducatproject.ducat.standCell
import org.ducatproject.ducat.standNow
import org.ducatproject.ducat.standStale
import uniffi.ducat_mobile.standEpoch
import uniffi.ducat_mobile.standEpochName
import uniffi.ducat_mobile.standEpochSecs
import uniffi.ducat_mobile.standShardName

/**
 * Boards rotate, so a poisoned one is abandoned rather than kept.
 * `./gradlew :desktop:generation`.
 *
 * A board's write key derives from its public name, so anybody can write any
 * slot — that was always the accepted cost of having no operator, and the
 * spec says so. What was not accepted, because nobody had noticed it, is that
 * the grief could be **permanent**: Veilid accepts an inbound write whenever
 * its sequence is merely *greater* than the stored one rather than exactly one
 * past it, and `ValueSeqNum::next()` fails at `u32::MAX - 1`. So one write per
 * slot at that maximum leaves a cell unwritable by anyone for ever, and the
 * record key is a pure function of the name — there is nowhere else to go.
 *
 * A generation in the name is what makes it recoverable. These cases are as
 * much about what must *not* change: a name already stamped keeps resolving to
 * the record it was written to, or a poster loses track of its own notice.
 */
fun main() {
    val base = "geo:u4pruy"

    // The epoch is floor division of a clock that is always an argument —
    // never read inside a decoder, or the vectors would rot on their own.
    check(standEpoch(0uL) == 0uL) { "GEN_FAIL epoch zero" }
    check(standEpoch(standEpochSecs() - 1uL) == 0uL) { "GEN_FAIL early rollover" }
    check(standEpoch(standEpochSecs()) == 1uL) { "GEN_FAIL late rollover" }
    check(standEpochSecs() == 604_800uL) { "GEN_FAIL a generation is a week" }

    // The name: epoch first, shard second, both decimal and unpadded. Two
    // spellings of one name are two record keys, which is a writer and a
    // reader standing at different corners.
    check(standEpochName(base, 3021uL) == "geo:u4pruy@3021") { "GEN_FAIL epoch name" }
    check(standShardName(standEpochName(base, 3021uL), 3u) == "geo:u4pruy@3021-3") {
        "GEN_FAIL epoch and shard do not compose"
    }

    // A name that already names a generation is left alone. This is the one
    // that matters for correctness rather than for defence: a poster reads
    // back, migrates and clears the board it *posted to*, and re-stamping
    // would silently point all three at a board its notice was never on.
    val stamped = standNow(base)
    check(stamped.startsWith("$base@")) { "GEN_FAIL standNow did not stamp: $stamped" }
    check(standNow(stamped) == stamped) { "GEN_FAIL re-stamping moved a live board" }
    check(standNow("$stamped-4") == "$stamped-4") { "GEN_FAIL re-stamping a shard moved it" }

    // Staleness, which is what makes a rollover cost one poll instead of one
    // refresh interval: the notice is still there, still unexpired, and
    // nobody is reading that board any more.
    check(!standStale(stamped)) { "GEN_FAIL this generation read as stale" }
    check(!standStale("$stamped-7")) { "GEN_FAIL a shard of this generation read as stale" }
    val old = standEpochName(base, standEpoch(0uL))
    check(standStale(old)) { "GEN_FAIL a board from 1970 read as current" }
    check(standStale("$old-2")) { "GEN_FAIL a shard of a dead generation read as current" }
    // A board name from before generations existed is the stalest thing
    // there is — nothing reads it now and nothing will again. An upgrading
    // device migrates its own live notices on the next poll because of this,
    // instead of finding out through a write that fails.
    check(standStale(base)) { "GEN_FAIL a pre-generation board read as current" }
    check(standStale("$base-3")) { "GEN_FAIL a pre-generation shard read as current" }

    // And what a person is shown is a place, not a bookkeeping detail.
    check(standCell("geo:u4pruy@3021-2") == "u4pruy") { "GEN_FAIL cell from a full name" }
    check(standCell("geo:u4pruy@3021") == "u4pruy") { "GEN_FAIL cell from shard zero" }
    check(standCell("local:u4pru@9") == "u4pru") { "GEN_FAIL cell from a listing board" }

    // The guard behind all of it: an unstamped name is not a board name, and
    // saying so out loud is what stops a forgotten call site from quietly
    // reading a board nobody else computes.
    val refused = runCatching { uniffi.ducat_mobile.standRecordKey(base) }
    check(refused.isFailure) { "GEN_FAIL an ungenerational board name was accepted" }
    check(refused.exceptionOrNull()?.message?.contains("generation") == true) {
        "GEN_FAIL the refusal does not say why: ${refused.exceptionOrNull()?.message}"
    }

    println(
        "GEN_OK epoch=floor week=604800 name=<cell>@<epoch>-<shard> restamp=noop " +
            "stale=detected legacy=stale display=cell-only unstamped=refused",
    )
}
