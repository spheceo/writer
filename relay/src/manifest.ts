/**
 * What Writer can do, for accounts.spheceo.com's admin pages (contract v2 in
 * accounts/docs/plan.md): served public at `/.well-known/spheceo-app.json`.
 * Admins grant these kinds of work; the relay itself only needs `use`, and
 * Writer gates each one itself from the token's `perms`.
 *
 * Ids are forever (`group.thing`, lowercase): rename a label, never an id.
 */

/** Writer's mark (public/favicon.svg). */
const MARK = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#0a0a0a"/><rect x="0.75" y="0.75" width="62.5" height="62.5" rx="13.25" fill="none" stroke="#262626" stroke-width="1.5"/><rect x="13" y="14" width="38" height="7.5" rx="3.75" fill="#ededed"/><rect x="13" y="28.25" width="29" height="7.5" rx="3.75" fill="#ededed"/><rect x="13" y="42.5" width="15" height="7.5" rx="3.75" fill="#ededed"/><rect x="33" y="40.25" width="12" height="12" rx="3" fill="#ff0033"/></svg>`;

export type Capability = { id: string; label: string; description: string; group: string };

export const CAPABILITIES: Capability[] = [
  { id: "scripts.interrogation", label: "Interrogation", description: "One case's footage (YouTube, Google Drive or this Mac) into an interrogation script.", group: "Scripts from footage" },
  { id: "scripts.interrogation-compilation", label: "Interrogation compilations", description: "2 to 20 cases in one interrogation script, standard (\"When…\") or hidden.", group: "Scripts from footage" },
  { id: "scripts.foia", label: "FOIA", description: "One case's FOIA footage (bodycam and records) into a FOIA script.", group: "Scripts from footage" },
  { id: "scripts.courtroom", label: "Courtroom", description: "One case's hearing into a courtroom script, including one hearing split across several videos.", group: "Scripts from footage" },
  { id: "scripts.courtroom-compilation", label: "Courtroom compilations", description: "Several cases' hearings in one courtroom script.", group: "Scripts from footage" },
  { id: "scripts.commentary", label: "Commentary", description: "A voiceover essay from articles, videos and Docs, with clips found and placed.", group: "Other scripts" },
  { id: "review.share", label: "Share for review", description: "Make review links, see and fix reviewers' notes, approve, and export to Google Docs.", group: "Sharing and teaching" },
  { id: "review.lessons", label: "Teach Writer from feedback", description: "Turn reviewers' feedback into lessons and keep them. Lessons are shared by every Mac and change everyone's builds.", group: "Sharing and teaching" },
];

export const MANIFEST = {
  app: "writer",
  version: 1,
  name: "Writer",
  icon: `data:image/svg+xml;base64,${btoa(MARK)}`,
  capabilities: CAPABILITIES,
  presets: [
    { id: "interrogation", label: "Interrogation writer", capabilities: ["scripts.interrogation", "review.share"] },
    { id: "compilation", label: "Compilation writer", capabilities: ["scripts.interrogation", "scripts.interrogation-compilation", "review.share"] },
    { id: "full", label: "All of Writer", capabilities: CAPABILITIES.map((capability) => capability.id) },
  ],
};

/** The manifest as served: public, any origin, cached up to an hour. */
export function manifestResponse(): Response {
  return new Response(JSON.stringify(MANIFEST, null, 2), {
    headers: {
      "content-type": "application/json",
      "access-control-allow-origin": "*",
      "cache-control": "public, max-age=3600",
    },
  });
}
