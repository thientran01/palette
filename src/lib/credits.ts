/*
 * Shapes credits.rs's role rows into the person-first layout Credits.tsx
 * draws: a few sections (Production, Songwriting, Performers, Engineering),
 * each listing people once with every role they had. K-pop pages repeat the
 * same names across roles (a producer who also wrote and arranged), so this
 * reads shorter than one line per role.
 */
import type { CreditRow } from "./backend";

export type CreditSection = "Production" | "Songwriting" | "Performers" | "Engineering";

export interface CreditPerson {
  name: string;
  roles: string[];
}

export interface CreditGroup {
  title: CreditSection;
  people: CreditPerson[];
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

export function groupCredits(rows: CreditRow[]): CreditGroup[] {
  const groups = new Map<CreditSection, Map<string, CreditPerson>>();
  for (const row of rows) {
    const [section, noun] = KNOWN[row.role] ?? [sectionOf(row.role), row.role];
    let people = groups.get(section);
    if (!people) groups.set(section, (people = new Map()));
    for (const name of row.names) {
      const key = name.toLowerCase();
      const person = people.get(key) ?? { name, roles: [] };
      if (!person.roles.includes(noun)) person.roles.push(noun);
      people.set(key, person);
    }
  }
  return ORDER.filter((t) => groups.has(t)).map((title) => ({
    title,
    people: [...groups.get(title)!.values()],
  }));
}
