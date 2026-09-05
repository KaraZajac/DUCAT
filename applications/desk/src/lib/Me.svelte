<script lang="ts">
  // Me: the name on your cards, the code somebody scans to reach you, and
  // the hats you wear.
  import { onMount } from "svelte";
  import { t, i18n, LANGS, applyLanguage } from "./i18n.svelte";
  import { api, copy, fmtWhen, type Code, type PersonaRow, type MyProfile } from "./api";
  import { gen, drive } from "./state.svelte";

  let personas = $state<PersonaRow[]>([]);
  let name = $state("");
  let savedName = $state<string | null>(null);
  let code = $state<Code | null>(null);
  let cutting = $state(false);
  let err = $state<string | null>(null);
  let newPersona = $state("");

  const worn = $derived(personas.find((p) => p.worn) ?? null);

  // Ways to reach you (§16.9): kept per persona, sent only on a profile
  // handshake and only while the switch is on. The shapes are the
  // phone's, so a value saved here reads the same there.
  let pEmail = $state(""), pPhone = $state(""), pSignal = $state(""), pShare = $state(false);
  let savedProfile = $state<MyProfile | null>(null);
  let profileMsg = $state<string | null>(null);
  let profileErr = $state<string | null>(null);
  const profileDirty = $derived(
    !!savedProfile &&
      (pEmail.trim() !== (savedProfile.email ?? "") ||
        pPhone.trim() !== (savedProfile.phone ?? "") ||
        pSignal.trim() !== (savedProfile.signal ?? "") ||
        pShare !== savedProfile.share),
  );
  function checkEmail(v: string): string | null {
    if (!v) return null;
    if (v.length > 254) return t("myprofile_email_too_long");
    const ok = /^[A-Za-z0-9._%+\-']+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}$/;
    if (!ok.test(v) || v.includes("..")) return t("myprofile_email_shape");
    return null;
  }
  function checkPhone(v: string): string | null {
    if (!v) return null;
    if (!/^[0-9]+$/.test(v)) return t("myprofile_phone_digits");
    if (v.length > 15) return t("myprofile_phone_too_long");
    return null;
  }
  function checkSignal(v: string): string | null {
    if (!v) return null;
    if (v.length > 48) return t("myprofile_signal_too_long");
    if (!/^[A-Za-z_][A-Za-z0-9_]{2,}\.[0-9]{2,}$/.test(v)) return t("myprofile_signal_shape");
    return null;
  }
  async function loadProfile() {
    try {
      const p = await api.myProfile();
      savedProfile = p;
      pEmail = p.email ?? ""; pPhone = p.phone ?? ""; pSignal = p.signal ?? ""; pShare = p.share;
    } catch (e) { profileErr = String(e); }
  }
  async function saveProfile() {
    profileErr = null; profileMsg = null;
    const email = pEmail.trim(), phone = pPhone.trim(), signal = pSignal.trim();
    const bad = checkEmail(email) ?? checkPhone(phone) ?? checkSignal(signal);
    if (bad) { profileErr = bad; return; }
    try {
      await api.setMyProfile({ email: email || null, phone: phone || null, signal: signal || null, share: pShare });
      profileMsg = t("myprofile_saved");
      await loadProfile();
    } catch (e) { profileErr = String(e); }
  }
  let passphrase = $state("");
  let backupMsg = $state<string | null>(null);
  let backupBusy = $state(false);
  let exportedAt = $state(0);
  let donate = $state<Code | null>(null);
  let theme = $state<string>((() => { try { return localStorage.getItem("ducat.theme") ?? "system"; } catch { return "system"; } })());

  function setTheme(v: string) {
    theme = v;
    try { localStorage.setItem("ducat.theme", v); } catch {}
    if (v === "light" || v === "dark") document.documentElement.dataset.theme = v;
    else delete document.documentElement.dataset.theme;
  }

  async function exportBackup(typed?: string) {
    err = null; backupMsg = null;
    const path = typed ?? (await api.pickSavePath("ducat-backup.ducat"));
    if (!path) return;
    backupBusy = true;
    try {
      const n = await api.exportBackup(path, passphrase);
      backupMsg = t("desk_written_to", n, path);
      await refresh();
    } catch (e) { err = String(e); } finally { backupBusy = false; }
  }

  async function importBackup(typed?: string) {
    err = null; backupMsg = null;
    const path = typed ?? (await api.pickFile());
    if (!path) return;
    backupBusy = true;
    try {
      const r = await api.importBackup(path, passphrase);
      backupMsg = t("desk_restored", r.contacts, r.personas, r.restore_height);
      code = null; name = "";
      await refresh();
    } catch (e) { err = String(e); } finally { backupBusy = false; }
  }

  async function refresh() {
    try {
      personas = await api.personas();
      exportedAt = (await api.backupInfo()).exported_at;
      const w = personas.find((p) => p.worn);
      savedName = w?.my_name ?? null;
      if (!name) name = savedName ?? "";
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => { refresh(); loadProfile(); });
  $effect(() => {
    void gen.value;
    refresh();
  });

  async function saveName() {
    err = null;
    try {
      await api.setMyName(name.trim());
      await refresh();
      // The code carries the name; a new name means a new code next time.
      code = null;
    } catch (e) {
      err = String(e);
    }
  }

  async function showCode() {
    err = null;
    cutting = true;
    try {
      code = await api.profileCode();
    } catch (e) {
      err = String(e);
    } finally {
      cutting = false;
    }
  }

  async function wear(hex: string) {
    await api.wear(hex);
    code = null;
    name = "";
    await refresh();
  }

  async function addPersona() {
    const n = newPersona.trim();
    if (!n) return;
    const p = await api.createPersona(n, 0);
    if (!p) err = t("personas_cap");
    newPersona = "";
    await refresh();
  }
</script>

<h1 class="page-title">{t("desk_nav_me")}</h1>
<p class="page-lede">{t("desk_me_lede")}</p>

<div class="card">
  <h3>{t("desk_your_name")}</h3>
  <div class="field">
    <label for="myname">{t("desk_on_your_cards")}</label>
    <input id="myname" class="input" bind:value={name} placeholder={t("desk_name_hint")} onkeydown={(e) => e.key === "Enter" && saveName()} />
    <button class="btn" onclick={saveName} disabled={name.trim() === (savedName ?? "")}>{t("myprofile_save")}</button>
  </div>
  <p class="note">{t("desk_name_note")}</p>
</div>

<div class="card">
  <h3>{t("desk_reach_title")}</h3>
  <p class="note">{t("myprofile_reach_note")}</p>
  <div class="field">
    <label for="pemail">{t("myprofile_email")}</label>
    <input id="pemail" class="input" type="email" bind:value={pEmail} placeholder="name@example.org" onkeydown={(e) => e.key === "Enter" && saveProfile()} />
  </div>
  <div class="field">
    <label for="pphone">{t("myprofile_phone")}</label>
    <input id="pphone" class="input" inputmode="numeric" bind:value={pPhone} placeholder={t("myprofile_phone_hint")} onkeydown={(e) => e.key === "Enter" && saveProfile()} />
  </div>
  <div class="field">
    <label for="psignal">{t("myprofile_signal")}</label>
    <input id="psignal" class="input" bind:value={pSignal} placeholder={t("myprofile_signal_hint")} onkeydown={(e) => e.key === "Enter" && saveProfile()} />
  </div>
  <label class="toggle"><input type="checkbox" bind:checked={pShare} /> {t("myprofile_share_switch")}</label>
  <p class="note">{pShare ? t("myprofile_share_on_note") : t("myprofile_share_off_note")}</p>
  <div class="actions">
    <button class="btn" onclick={saveProfile} disabled={!profileDirty}>{t("myprofile_save")}</button>
    {#if profileMsg}<span class="meta">{profileMsg}</span>{/if}
  </div>
  {#if profileErr}<p class="err">{profileErr}</p>{/if}
</div>

<div class="card">
  <h3>{t("qrhub_my_code")}</h3>
  {#if code}
    <div class="code-wrap">
      <div class="qr">{@html code.svg}</div>
      <div class="code-side">
        <p class="note">{t("desk_code_note")}</p>
        <div class="addr">{code.uri}</div>
        <div class="actions">
          <button class="btn small" onclick={() => copy(code?.uri ?? "")}>{t("desk_copy_code")}</button>
          <button class="btn small" onclick={showCode}>{t("desk_refresh")}</button>
        </div>
      </div>
    </div>
  {:else}
    <button class="btn primary" onclick={showCode} disabled={cutting}>{cutting ? t("desk_cutting") : t("desk_show_my_code")}</button>
    <p class="note">{t("desk_code_cost")}</p>
  {/if}
</div>

<div class="card">
  <h3>{t("desk_appearance")}</h3>
  <div class="field">
    <label for="theme">{t("desk_theme")}</label>
    <select id="theme" class="input narrow" value={theme} onchange={(e) => setTheme((e.target as HTMLSelectElement).value)}>
      <option value="system">{t("desk_language_system")}</option>
      <option value="light">{t("desk_theme_light")}</option>
      <option value="dark">{t("desk_theme_dark")}</option>
    </select>
  </div>
  <div class="field">
    <label for="lang">{t("desk_language")}</label>
    <select id="lang" class="input" value={i18n.choice} onchange={(e) => applyLanguage((e.target as HTMLSelectElement).value)}>
      <option value="">{t("desk_language_system")}</option>
      {#each LANGS as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
    </select>
  </div>
  <p class="note">{t("desk_language_note")}</p>
</div>

<div class="card">
  <h3>{t("activity_donations_filter")}</h3>
  <p class="note">{t("desk_donations_note")}</p>
  {#if donate}
    <div class="code-wrap">
      <div class="qr">{@html donate.svg}</div>
      <div class="code-side"><div class="addr">{donate.uri}</div><div class="actions"><button class="btn small" onclick={() => copy(donate?.uri ?? "")}>{t("desk_copy_code")}</button></div></div>
    </div>
  {:else}
    <button class="btn" onclick={async () => { try { donate = await api.donateCode(); } catch (e) { err = String(e); } }}>{t("desk_show_donation_code")}</button>
  {/if}
</div>

<div class="card">
  <h3>{t("backup_title")}</h3>
  <p class="note">{t("desk_backup_note")} {#if exportedAt}{t("desk_last_exported", fmtWhen(exportedAt))}{:else}{t("desk_never_exported")}{/if}</p>
  <div class="field">
    <label for="pass">{t("backup_passphrase")}</label>
    <input id="pass" class="input" type="password" bind:value={passphrase} placeholder={t("desk_passphrase_hint")} />
    <button class="btn" disabled={passphrase.length < 8 || backupBusy} onclick={() => exportBackup()}>{t("backup_export")}…</button>
    <button class="btn" disabled={passphrase.length < 8 || backupBusy} onclick={() => importBackup()}>{t("backup_import")}…</button>
        {#if drive.on}
          <input id="bpath" class="input" hidden placeholder="/path/to/export.ducat" onchange={(e) => exportBackup((e.target as HTMLInputElement).value)} />
          <input id="ipath" class="input" hidden placeholder="/path/to/import.ducat" onchange={(e) => importBackup((e.target as HTMLInputElement).value)} />
        {/if}
  </div>
  {#if backupMsg}<p class="note ok-text">{backupMsg}</p>{/if}
</div>

<div class="card">
  <h3>{t("desk_personas")}</h3>
  <p class="note">{t("desk_personas_note")}</p>
  {#each personas as p (p.hex)}
    <div class="row">
      <div class="lead">
        <div class="title">{p.name || (p.primary ? t("personas_primary") : t("desk_unnamed"))} {#if p.worn}<span class="meta">· {t("personas_worn").toLowerCase()}</span>{/if}</div>
        <div class="meta">{p.my_name ? t("desk_cards_say", p.my_name) : t("desk_no_name_yet")} · <span class="mono">{p.hex.slice(0, 16)}…</span></div>
      </div>
      <div class="actions">
        {#if !p.worn}<button class="btn small" onclick={() => wear(p.hex)}>{t("desk_wear")}</button>{/if}
      </div>
    </div>
  {/each}
  {#if personas.length < 4}
    <div class="field">
      <label for="newp">{t("personas_add")}</label>
      <input id="newp" class="input" bind:value={newPersona} placeholder={t("personas_name_support")} onkeydown={(e) => e.key === "Enter" && addPersona()} />
      <button class="btn" onclick={addPersona} disabled={!newPersona.trim()}>{t("desk_create")}</button>
    </div>
  {/if}
  {#if err}<p class="err">{err}</p>{/if}
</div>
