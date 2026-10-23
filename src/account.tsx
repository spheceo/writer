import { useState, useSyncExternalStore } from "react";
import { type AccountStatus, accountStatus, cancelSignIn, message, openPage, signIn, signInWithCode, signOut } from "./api";
import { Avatar } from "./avatar";
import mark from "./assets/writer-mark.svg";
import { Menu } from "./ui";

/** Who's signed in, shared by every part of the page: asked again every few
 * seconds while the page is in view (a sign-in waiting on the browser, a
 * renewal that changed someone's grants), every 30 s when it isn't. */
let current: AccountStatus | null = null;
const listeners = new Set<() => void>();
let polling = false;
function publish(next: AccountStatus) {
  current = next;
  for (const listener of listeners) listener();
}
function poll() {
  void accountStatus()
    .then(publish, () => {})
    .finally(() => window.setTimeout(poll, document.hidden ? 30_000 : 3_000));
}
function subscribe(listener: () => void) {
  listeners.add(listener);
  if (!polling) {
    polling = true;
    poll();
  }
  return () => listeners.delete(listener);
}

export function useAccount() {
  const status = useSyncExternalStore(subscribe, () => current);
  return [status, publish] as const;
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

/** Google's mark, for "Continue with Google". */
function GoogleMark() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z" />
      <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" />
      <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" />
      <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" />
    </svg>
  );
}

/** Setup's quiet account line: who's signed in, and what they may use. */
export function AccountRow() {
  const [status] = useAccount();
  const person = status?.person;
  if (!status?.signedIn) return null;
  return (
    <p className="faint account-line" title={person ? `May use ${allowedText(person.perms, person.admin)}.` : undefined}>
      {person ? `Signed in as ${person.email}` : "Signed in (offline)"}
    </p>
  );
}

/** Your avatar at the far right of the header, and the account menu it opens. */
export function AccountMenu({ onSettings, onLogs }: { onSettings: () => void; onLogs: () => void }) {
  const [status, setStatus] = useAccount();
  const person = status?.person;
  if (!status?.signedIn || !person) return null;
  const me = { sub: person.sub, name: person.name, email: person.email, picture: person.image };
  return (
    <Menu
      className="account-menu"
      label="Your account"
      trigger={<Avatar creator={me} size={32} label={`${person.name || person.email}: your account`} />}
      head={
        <div className="menu-who">
          <Avatar creator={me} size={32} label={person.name || person.email} />
          <span>
            <b>{person.name || person.email}</b>
            <small>{person.email}</small>
          </span>
        </div>
      }
      items={[
        { label: "Manage account", onSelect: () => void openPage("https://accounts.spheceo.com") },
        { label: "Setup…", hint: "⌘,", onSelect: onSettings, separated: true },
        { label: "Logs", hint: "⇧⌘L", onSelect: onLogs },
        { label: "Report a problem…", onSelect: () => void openPage("https://github.com/spheceo/writer/issues/new") },
        { label: "Sign out", separated: true, onSelect: () => void signOut().then(setStatus, () => {}) },
      ]}
    />
  );
}

/** Signed out: the whole window is the sign-in (the kit's full-window sign-in), until someone signs in. */
export function SignInScreen() {
  const [status, setStatus] = useAccount();
  const [problem, setProblem] = useState<string | null>(null);
  if (!status || status.signedIn) return null;
  const said = problem ?? status.error;
  const code = status.code;
  return (
    <main className="signin" data-tauri-drag-region>
      <div className="signin-card">
        <img className="signin-mark" src={mark} alt="" width={56} height={56} />
        <h1>Sign in to Writer</h1>
        {code ? (
          <>
            <p>On any device, open {code.url.replace(/^https:\/\//, "")} and enter this code. Writer signs in as soon as you agree.</p>
            <div className="signin-code mono" aria-label="Your code">{code.userCode}</div>
            <button type="button" className="secondary lg block" onClick={() => void cancelSignIn().then(setStatus, () => {})}>
              Cancel
            </button>
          </>
        ) : (
          <>
            <p>Use the Google account an admin added. Your browser opens to finish, then brings you back to Writer.</p>
            {status.signingIn ? (
              <button type="button" className="secondary lg block" onClick={() => void cancelSignIn().then(setStatus, () => {})}>
                Waiting for the browser… Cancel
              </button>
            ) : (
              <button
                type="button"
                className="google lg block"
                onClick={() => {
                  setProblem(null);
                  void signIn().then(setStatus, (cause) => setProblem(message(cause)));
                }}
              >
                <GoogleMark />
                Continue with Google
              </button>
            )}
            <p className="signin-alt">
              On another device?{" "}
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setProblem(null);
                  void signInWithCode().then(setStatus, (cause) => setProblem(message(cause)));
                }}
              >
                Sign in with a code
              </button>
            </p>
          </>
        )}
        {said && <p className="bad signin-problem">{said}</p>}
        <p className="signin-foot">Only people an admin has added can sign in.</p>
      </div>
    </main>
  );
}
