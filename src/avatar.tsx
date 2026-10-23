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

/** Who started a script: their Google photo, or their initials; nothing for a script with no creator. */
export function Avatar({ creator, size = 20 }: { creator?: Creator | null; size?: number }) {
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
  const label = `Started by ${creator.name || creator.email || "someone"}`;
  return (
    <span className="avatar" title={label} aria-label={label} style={{ width: size, height: size, fontSize: Math.round(size * 0.42) }}>
      {src && !broken ? <img src={src} alt="" referrerPolicy="no-referrer" onError={() => setBroken(true)} /> : initials(creator)}
    </span>
  );
}
