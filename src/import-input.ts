/** Only a card pasted on its own replaces the form with that card's cases.
 * A REUSE line may name an earlier card without importing that entire card. */
export function pastedTrelloCard(text: string) {
  return /^https?:\/\/(?:www\.)?trello\.com\/c\/[A-Za-z0-9]+[^\s]*$/.exec(text.trim())?.[0] ?? null;
}
