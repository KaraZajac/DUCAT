package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Badge
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Persona
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R

/**
 * The compartments, on screen (post-1.0 track).
 *
 * Two rules from the design note carried into the UI: the persona choice
 * exists only at doorways — everything else *displays* a binding it cannot
 * change — and the roster stays small enough to fit on one hand, so this
 * screen is a short list, not a manager.
 */

/** The accents a persona can wear. Mocha's pastels read on both themes. */
private val ACCENTS = listOf(
    0xFFCBA6F7.toInt(), // mauve
    0xFFFAB387.toInt(), // peach
    0xFFA6E3A1.toInt(), // green
    0xFF89DCEB.toInt(), // sky
    0xFFF5C2E7.toInt(), // pink
    0xFFF9E2AF.toInt(), // yellow
)

/** What to call a persona: its name, or the word for the unnamed primary. */
@Composable
fun personaLabel(p: Persona): String =
    p.name.ifBlank { stringResource(R.string.personas_primary) }

/** The accent dot; the primary's default (0) wears the theme's own accent. */
@Composable
fun PersonaDot(p: Persona, size: Int = 10) {
    val color = if (p.color != 0) Color(p.color) else MaterialTheme.colorScheme.primary
    Spacer(
        Modifier
            .size(size.dp)
            .background(color, CircleShape),
    )
}

/**
 * The switcher: one chip per persona, the worn one filled. Lives in the
 * drawer header — putting on a hat is a deliberate act, not a top-bar
 * accident waiting beside the compose button.
 */
@Composable
fun PersonaSwitcher(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val store = remember { PersonaStore(context) }
    val personas = remember(version) { store.all() }
    if (personas.size < 2) return
    val worn = remember(version) { store.worn() }
    // Which hats have a conversation waiting. The Chat tab's badge counts
    // every compartment and the list shows one, so a "4" over a list with
    // a single dot left the other three findable only by trying each chip
    // in turn; the mark belongs where the other compartments are reached.
    // Off the main thread — this decrypts the contact book, and the drawer
    // header is composed whether or not the drawer is open.
    val waiting by produceState(emptyMap<String, Int>(), version) {
        value = withContext(Dispatchers.IO) {
            val contacts = ContactStore(context)
            val threads = contacts.all()
                .filter { it.chatVisible && it.inSeq > contacts.chatSeen(it) }
                .groupingBy { store.ownerHexOf(it) }
                .eachCount()
            // Plus the groups, under the hat that sits in each — the tab
            // badge counts them, so the chips have to.
            val groups = org.ducatproject.ducat.Groups.unreadGroupsByOwner(context)
            (threads.keys + groups.keys).associateWith {
                (threads[it] ?: 0) + (groups[it] ?: 0)
            }
        }
    }
    val unreadLabel = stringResource(R.string.chatlist_unread)
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (p in personas) {
            val unread = waiting[p.hex] ?: 0
            FilterChip(
                selected = p.hex == worn,
                onClick = { store.setWorn(p.hex) },
                leadingIcon = { PersonaDot(p) },
                label = { Text(personaLabel(p)) },
                // The Chat tab's badge, on the hat, so the two add up. Not
                // the list's dot: the primary's accent is the same colour,
                // and "Personal" between two identical dots read as trim.
                // The count is for the eye; a reader gets the word, merged
                // into the chip's label ahead of the selected state it
                // already announces.
                trailingIcon = if (unread > 0) {
                    {
                        Badge(Modifier.semantics { contentDescription = unreadLabel }) {
                            Text(org.ducatproject.ducat.Amounts.count(unread.toLong()))
                        }
                    }
                } else null,
            )
        }
    }
}

/**
 * The Settings section: the roster, a rename, and the one way to grow it.
 * No delete on purpose — the store's rule, said here where somebody would
 * look for the button.
 */
@Composable
internal fun NewProfileDialog(onDone: (String?) -> Unit) {
    val context = LocalContext.current
    val store = remember { PersonaStore(context) }
    // Both saveable: the caller's flag survives a rotation now, so the
    // dialog comes back — and it used to come back empty, over a name and a
    // colour somebody had just chosen.
    var name by rememberSaveable { mutableStateOf("") }
    var color by rememberSaveable { mutableStateOf(ACCENTS.first()) }
    androidx.compose.material3.AlertDialog(
        onDismissRequest = { onDone(null) },
        title = { Text(stringResource(R.string.personas_add)) },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = { if (it.length <= 24) name = it },
                    label = { Text(stringResource(R.string.personas_name_label)) },
                    supportingText = { Text(stringResource(R.string.personas_name_support)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (c in ACCENTS) {
                        Spacer(
                            Modifier
                                .size(28.dp)
                                .background(Color(c), CircleShape)
                                .then(
                                    if (c == color) Modifier.border(
                                        2.dp, MaterialTheme.colorScheme.onSurface, CircleShape,
                                    ) else Modifier,
                                )
                                .clickable { color = c },
                        )
                    }
                }
                Spacer(Modifier.height(10.dp))
                Text(
                    stringResource(R.string.personas_no_delete),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        },
        confirmButton = {
            TextButton(
                enabled = name.isNotBlank(),
                onClick = {
                    val p = store.create(name.trim(), color)
                    ContactStore.bump()
                    onDone(p?.hex)
                },
            ) { Text(stringResource(R.string.pub_create)) }
        },
        dismissButton = {
            TextButton(onClick = { onDone(null) }) {
                Text(stringResource(R.string.personas_cancel))
            }
        },
    )
}
