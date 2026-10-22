import type { strip as ru } from "../ru/strip";

export const strip: typeof ru = {
  region: "Screenshot set",
  moveHint: "Drag to move the strip",
  noComments: "no comments",
  snapshot: "Screenshot {letter}",
  remove: "Remove screenshot {letter} from the set",
  copyAll: "Copy all",
  copyAllHint:
    "Copy every screenshot as files: one paste sends them as one album. Comments drawn on the images go with them; to send separate comment text too, use paste with text",
  newSnapshot: "New screenshot",
  newSnapshotHint: "Take the next screenshot for this set",
  saveAll: "Save all",
  saveAllHint: "Save every screenshot to the screenshots folder",
  clear: "Clear",
  clearHint: "Remove all screenshots from the strip",
  clearConfirm: "Remove all?",
  clearCancel: "Keep",
  smartPaste: "paste with text",
  smartPasteHint: "Pastes every screenshot and the comment text into the open chat in one go",
  smartPasteOff: "The {hotkey} shortcut did not register: another app may be using it. Click to change it in Settings.",
  failed: "That did not work, please try again",
};
