import { useEffect, useState } from "react";
import { type AccountStatus, accountStatus, cancelSignIn, message, signIn, signOut } from "./api";
import { Dialog } from "./ui";

/** Who's signed in, asked again every few seconds while a sign-in waits on the browser. */
export function useAccount() {
  const [status, setStatus] = useState<AccountStatus | null>(null);
  useEffect(() => {
    let live = true;
    let timer = 0;
    const ask = () =>
      void accountStatus()
        .then((next) => live && setStatus(next), () => {})
        .finally(() => {
          if (live) timer = window.setTimeout(ask, document.hidden ? 30_000 : 3_000);
        });
    ask();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, []);
  return [status, setStatus] as const;
}

const PERMISSIONS: Record<string, string> = {
  use: "transcription",
  ai: "the watcher",
  drive: "Drive and Docs",
  trello: "Trello",
  proxy: "YouTube downloads",
  cloud: "the cloud",
};

/** What a person may use, in words. */
function allowedText(perms: string[], admin: boolean) {
  if (admin) return "an admin: everything";
  const named = perms.filter((perm) => perm !== "use" || perms.length === 1).map((perm) => PERMISSIONS[perm] ?? perm);
  return named.length ? named.join(", ") : "nothing yet; ask an admin";
}

function SignInButton({ status, onChange, className = "primary" }: { status: AccountStatus; onChange: (next: AccountStatus) => void; className?: string }) {
  const [problem, setProblem] = useState<string | null>(null);
  if (status.signingIn) {
    return (
      <button type="button" className="ghost" onClick={() => void cancelSignIn().then(onChange, () => {})}>
        Waiting for the browser… Cancel
      </button>
    );
  }
  return (
    <>
      {problem && <span className="bad">{problem}</span>}
      <button
        type="button"
        className={className}
        onClick={() => {
          setProblem(null);
          void signIn().then(onChange, (cause) => setProblem(message(cause)));
        }}
      >
        Sign in with Google
      </button>
    </>
  );
}

/** Setup's account row: "Signed in as …" and Sign out, or Sign in. */
export function AccountRow() {
  const [status, setStatus] = useAccount();
  const [busy, setBusy] = useState(false);
  if (!status) return null;
  const person = status.person;
  return (
    <div className="update-channel">
      <b>Account</b>
      {status.signedIn ? (
        <>
          <span className="faint" title={person ? `${person.name} <${person.email}>\nMay use ${allowedText(person.perms, person.admin)}.` : undefined}>
            {person ? `Signed in as ${person.email}` : "Signed in (offline)"}
          </span>
          <button
            type="button"
            className="ghost sm"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void signOut().then(setStatus, () => {}).finally(() => setBusy(false));
            }}
          >
            {busy ? "Signing out…" : "Sign out"}
          </button>
        </>
      ) : (
        <>
          <span className={status.error ? "bad" : "faint"}>{status.error ?? "Not signed in."}</span>
          <SignInButton status={status} onChange={setStatus} className="ghost sm" />
        </>
      )}
    </div>
  );
}

/** Signed out (at launch, or when a sign-in ends): Writer needs someone signed in at accounts.spheceo.com. */
export function SignInDialog() {
  const [status, setStatus] = useAccount();
  if (!status) return null;
  return (
    <Dialog
      open={!status.signedIn}
      onClose={() => {}}
      title="Sign in to Writer"
      description="Sign in with the Google account an admin added at accounts.spheceo.com. Your browser opens to do it."
      width={420}
      footer={
        <>
          <span className="spacer" />
          <SignInButton status={status} onChange={setStatus} />
        </>
      }
    >
      {status.error && <p className="bad">{status.error}</p>}
    </Dialog>
  );
}
