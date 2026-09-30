export interface MultilineKeyGesture {
  key: string;
  isComposing: boolean;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}

export function shouldSubmitMultilineGesture(
  gesture: MultilineKeyGesture,
  submitOnEnter: boolean,
): boolean {
  if (gesture.key !== "Enter" || gesture.isComposing) return false;
  if (submitOnEnter) return !gesture.shiftKey;
  return gesture.ctrlKey || gesture.metaKey;
}
