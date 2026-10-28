/** Older imports saved the full transport error, including the request URL. */
export function importError(error?: string | null): string {
  if (error?.startsWith("The connection to Writer's service broke (")) {
    return "The connection to Writer's service closed before a transcription answer arrived. Retry to continue from the saved parts. Technical details are in Logs on the Mac that ran the import.";
  }
  return error || "It didn't finish.";
}
