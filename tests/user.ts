import userEvent from "@testing-library/user-event";

/**
 * A user-event session with no inter-key delay.
 *
 * The dialogs are controlled forms that re-render on every keystroke, and
 * user-event's default delay turns a fifteen-character hostname into seconds
 * of test time. Nothing under test depends on the timing between keys.
 */
export function typing() {
  return userEvent.setup({ delay: null });
}
