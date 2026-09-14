import { t } from "./i18n.svelte";

/** §9.2: "Pat knows them", "Pat and Sam know them", "3 of your contacts know them" — or nothing. */
export function knownWords(names: string[]): string {
  if (names.length === 0) return "";
  if (names.length === 1) return t("desk_known_by_one", names[0]);
  if (names.length === 2) return t("desk_known_by_names", names.join(t("desk_and")));
  return t("desk_known_by_count", String(names.length));
}
