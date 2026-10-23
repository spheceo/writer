import { useEffect, useState } from "react";
import { avatarSrc } from "./api";
import type { Creator } from "./types";

/** Each person's photo, asked for once per page (Writer keeps the file). */
const photos = new Map<string, Promise<string | null>>();
function photoOf(creator: Creator) {
  const key = `${creator.sub}\n${creator.picture}`;
  if (!photos.has(key)) photos.set(key, creator.picture ? avatarSrc(creator.sub, creator.picture).catch(() => null) : Promise.resolve(null));
  return photos.get(key)!;
}

/** Two letters from the name, or the email's first. */
export function initials(creator: Creator) {
  const words = creator.name.trim().split(/\s+/).filter(Boolean);
  if (words.length) return (words[0][0] + (words.length > 1 ? words[words.length - 1][0] : "")).toUpperCase();
  return (creator.email?.[0] ?? "?").toUpperCase();
}

/** One of the kit's eight avatar hues, by a stable hash of the name (FNV-1a), so a person is the same colour in every app. */
export function hue(creator: Creator) {
  let hash = 0x811c9dc5;
  for (const char of (creator.name.trim() || creator.email || creator.sub).toLowerCase()) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return (hash % 8) + 1;
}

/** A person: their Google photo over their initials (in their hue); nothing when there's no one. */
export function Avatar({ creator, size = 24, label: said }: { creator?: Creator | null; size?: number; label?: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [broken, setBroken] = useState(false);
  useEffect(() => {
    setSrc(null);
    setBroken(false);
    if (!creator) return;
    let live = true;
    void photoOf(creator).then((found) => live && setSrc(found));
    return () => {
      live = false;
    };
  }, [creator?.sub, creator?.picture]);
  if (!creator) return null;
  const label = said ?? `Started by ${creator.name || creator.email || "someone"}`;
  return (
    <span className="person" title={label} aria-label={label} style={{ width: size, height: size, fontSize: Math.round(size * 0.4), ["--hue" as string]: `var(--avatar-${hue(creator)})` }}>
      {initials(creator)}
      {src && !broken && <img src={src} alt="" referrerPolicy="no-referrer" onError={() => setBroken(true)} />}
    </span>
  );
}
