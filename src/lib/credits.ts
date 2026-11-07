/*
 * Shapes credits.rs's role rows into the person-first layout Credits.tsx
 * draws: a few sections (Production, Songwriting, Performers, Engineering).
 * Within a section each person is merged across roles, then people with the
 * same set of roles share one line ("Lyrics · Composition: A, B, C"). Genius
 * lists every contributor, and a K-pop page can name a dozen writers, so a
 * line per person never fit the compact panel (Thien, 2026-11-07).
 */
import type { CreditRow } from "./backend";

export type CreditSection = "Production" | "Songwriting" | "Performers" | "Engineering";

/** A credited name. `aside` is a parenthetical shown quieter: the
 * romanization in "구종필 (JongPil Gu)" or a disambiguator like "(KOR)". */
export interface CreditName {
  main: string;
  aside: string | null;
}

/** People who share exactly the same roles, on one line. */
export interface CreditLine {
  roles: string[];
  names: CreditName[];
}

export interface CreditGroup {
  title: CreditSection;
  lines: CreditLine[];
}

const ORDER: CreditSection[] = ["Production", "Songwriting", "Performers", "Engineering"];

/** credits.rs labels → [section, the role as a noun under a person's name]. */
const KNOWN: Record<string, [CreditSection, string]> = {
  "Produced by": ["Production", "Producer"],
  "Co-produced by": ["Production", "Co-producer"],
  "Vocal direction": ["Production", "Vocal direction"],
  "Written by": ["Songwriting", "Writer"],
  Lyrics: ["Songwriting", "Lyrics"],
  Composition: ["Songwriting", "Composition"],
  Arrangement: ["Songwriting", "Arrangement"],
  Vocals: ["Performers", "Vocals"],
  "Background vocals": ["Performers", "Background vocals"],
  "Recorded by": ["Engineering", "Recording"],
  "Mixed by": ["Engineering", "Mixing"],
  "Mastered by": ["Engineering", "Mastering"],
};

/** Where a role credits.rs passed through as-is (e.g. "Digital editor",
 * "Bass") belongs. Instruments are the common case, so they're the default. */
function sectionOf(role: string): CreditSection {
  const r = role.toLowerCase();
  if (/engineer|edit|recording|mix|master/.test(r)) return "Engineering";
  if (/produc|programming|direct/.test(r)) return "Production";
  if (/writ|lyric|compos|arrang/.test(r)) return "Songwriting";
  return "Performers";
}

const fold = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");

/** "구종필 (JongPil Gu)" → main "구종필", aside "JongPil Gu". Every
 * trailing parenthetical goes to the aside ("진동욱 (the orchard) (KOR)" →
 * "the orchard KOR"), except a tag that only repeats the playing artist
 * ("Gawon (MEOVV)" while MEOVV plays), which is dropped. */
export function splitName(name: string, artist: string): CreditName {
  const m = /^([^()]*[^()\s])\s*((?:\([^()]*\)\s*)+)$/.exec(name.trim());
  if (!m) return { main: name.trim(), aside: null };
  const a = fold(artist);
  const tags = [...m[2].matchAll(/\(([^()]*)\)/g)]
    .map((t) => t[1].trim())
    .filter((t) => {
      const f = fold(t);
      return f && !(a && (a === f || a.includes(f) || f.includes(a)));
    });
  return { main: m[1], aside: tags.length ? tags.join(" ") : null };
}

export function groupCredits(rows: CreditRow[], artist = ""): CreditGroup[] {
  // section → person key → { name, roles }, in first-seen order.
  const groups = new Map<CreditSection, Map<string, { name: CreditName; roles: string[] }>>();
  for (const row of rows) {
    const [section, noun] = KNOWN[row.role] ?? [sectionOf(row.role), row.role];
    let people = groups.get(section);
    if (!people) groups.set(section, (people = new Map()));
    for (const raw of row.names) {
      const name = splitName(raw, artist);
      const key = fold(name.main) || name.main;
      const person = people.get(key) ?? { name, roles: [] };
      if (!person.roles.includes(noun)) person.roles.push(noun);
      people.set(key, person);
    }
  }
  return ORDER.filter((t) => groups.has(t)).map((title) => {
    const lines = new Map<string, CreditLine>();
    for (const p of groups.get(title)!.values()) {
      const key = p.roles.join("\u0001");
      const line = lines.get(key) ?? { roles: p.roles, names: [] };
      line.names.push(p.name);
      lines.set(key, line);
    }
    return { title, lines: [...lines.values()] };
  });
}
