import { useAccount } from "./account";
import type { Kind, Script, Summary } from "./types";

/**
 * What the signed-in person may do in Writer, from their grants at
 * accounts.spheceo.com (the same rules as src-tauri/src/can.rs, which
 * enforces them; this only decides what's shown).
 */
export const CAN = {
  interrogation: "scripts.interrogation",
  interrogationCompilation: "scripts.interrogation-compilation",
  foia: "scripts.foia",
  courtroom: "scripts.courtroom",
  courtroomCompilation: "scripts.courtroom-compilation",
  commentary: "scripts.commentary",
  share: "review.share",
  lessons: "review.lessons",
} as const;

export type Grants = { has: (id: string) => boolean; singleKinds: Kind[]; compilationKinds: Kind[] };

export function grantsOf(perms: string[], admin: boolean): Grants {
  const has = (id: string) => admin || perms.includes(id);
  return {
    has,
    singleKinds: (
      [
        ["interrogation", CAN.interrogation],
        ["foia", CAN.foia],
        ["courtroom", CAN.courtroom],
      ] as const
    )
      .filter(([, id]) => has(id))
      .map(([kind]) => kind),
    // FOIA has no compilation form.
    compilationKinds: (
      [
        ["interrogation", CAN.interrogationCompilation],
        ["courtroom", CAN.courtroomCompilation],
      ] as const
    )
      .filter(([, id]) => has(id))
      .map(([kind]) => kind),
  };
}

/** The signed-in person's grants (none until the account is known). */
export function useGrants(): Grants {
  const [status] = useAccount();
  const person = status?.person;
  return grantsOf(person?.perms ?? [], person?.admin ?? false);
}

/** The kind of work a script is, as can.rs names it. */
export function workOf(script: Pick<Summary | Script, "sourceKind" | "compilation" | "kind">): string {
  if (script.sourceKind === "commentary") return CAN.commentary;
  const kind = script.kind ?? "interrogation";
  if (script.compilation) return kind === "courtroom" ? CAN.courtroomCompilation : CAN.interrogationCompilation;
  return kind === "foia" ? CAN.foia : kind === "courtroom" ? CAN.courtroom : CAN.interrogation;
}
